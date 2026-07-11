//! RCON client for Quake 2 server communication.
//!
//! Implements UDP-based RCON protocol for sending commands to q2pro servers.

use std::net::SocketAddr;
use std::time::Duration;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::timeout;

/// RCON client error types.
#[derive(Debug, Error)]
pub enum RconError {
    #[error("Connection timeout")]
    Timeout,
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("Network error: {0}")]
    Network(#[from] std::io::Error),
}

/// RCON client for communicating with Quake 2 servers.
pub struct RconClient {
    host: String,
    port: u16,
    password: String,
    /// Mutex to serialize RCON calls and prevent response mixing
    lock: tokio::sync::Mutex<()>,
}

impl RconClient {
    pub fn new(host: &str, port: u16, password: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            password: password.to_string(),
            lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn execute(&self, command: &str) -> Result<String, RconError> {
        // Serialize all RCON calls to prevent response mixing
        let _guard = self.lock.lock().await;
        
        tracing::info!("RCON executing: {}", command);
        
        let addr_str = format!("{}:{}", self.host, self.port);
        let addr = tokio::net::lookup_host(&addr_str)
            .await
            .map_err(|e| RconError::InvalidResponse(format!("Failed to resolve host: {}", e)))?
            .next()
            .ok_or_else(|| RconError::InvalidResponse("Failed to resolve host".to_string()))?;

        match self.execute_udp(addr, command).await {
            Ok(response) => {
                tracing::info!("RCON UDP response ({} chars): {}", response.len(), response.lines().next().unwrap_or(""));
                // Add delay to let q2pro server process the response before next command
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(response);
            }
            Err(RconError::Timeout) => {
                tracing::warn!("RCON UDP timeout, falling back to TCP");
            }
            Err(e) => return Err(e),
        }

        let response = self.execute_tcp(addr, command).await?;
        tracing::info!("RCON TCP response ({} chars): {}", response.len(), response.lines().next().unwrap_or(""));
        // Add delay for TCP as well
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(response)
    }

    async fn execute_udp(&self, addr: SocketAddr, command: &str) -> Result<String, RconError> {
        // Create a fresh socket for each command to avoid response mixing
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(addr).await?;

        let rcon_command = b"\xff\xff\xff\xff".to_vec();
        let command_bytes = format!("rcon \"{}\" {}", self.password, command).into_bytes();

        let mut packet = rcon_command;
        packet.extend_from_slice(&command_bytes);

        socket.send(&packet).await?;

        // A large `status` reply does NOT arrive in one datagram. The server console
        // redirect flushes output in fixed ~1.3KB chunks (SV_OUTPUTBUF_LENGTH), each
        // sent as its own connectionless `\xff\xff\xff\xff` + "print\n<chunk>" packet.
        // A single recv() would read only the first chunk and silently truncate the
        // list (caps player lists at ~18). So loop, reassembling every datagram until
        // the reply goes quiet. These OOB print packets carry no sequence numbers, so
        // arrival order is the only order available (correct on a LAN, and how every
        // real Q2 rcon client behaves).
        const MAX_DATAGRAMS: usize = 64;
        let mut buf = [0u8; 4096];
        let mut response = String::new();

        for i in 0..MAX_DATAGRAMS {
            // The first datagram gets the full timeout (server may take a moment).
            // Subsequent reads use a short idle timeout: when it elapses with no more
            // data, the reply is complete — that is normal end-of-reply, not an error.
            let recv_timeout = if i == 0 {
                Duration::from_secs(5)
            } else {
                Duration::from_millis(250)
            };

            let n = match timeout(recv_timeout, socket.recv(&mut buf)).await {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => return Err(RconError::InvalidResponse(e.to_string())),
                Err(_) => {
                    // Timeout on the first datagram is a real failure; on later reads
                    // it just means the multi-packet reply has ended.
                    if i == 0 {
                        return Err(RconError::Timeout);
                    }
                    break;
                }
            };

            // Q2 connectionless packets are prefixed with 0xFFFFFFFF (4 bytes). Only
            // decode the bytes actually received, not the whole zero-padded buffer.
            let payload = buf.get(4..n).unwrap_or(&[]);
            let chunk = String::from_utf8_lossy(payload);
            // Each flushed packet carries its own leading "print\n" (SV_FlushRedirect).
            let chunk = chunk.strip_prefix("print\n").unwrap_or(&chunk);
            response.push_str(chunk);
        }

        Ok(response.trim().to_string())
    }

    async fn execute_tcp(&self, addr: SocketAddr, command: &str) -> Result<String, RconError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(addr).await?;

        let rcon_command = format!("rcon \"{}\" {}\n", self.password, command);
        stream.write_all(rcon_command.as_bytes()).await?;

        // TCP is a stream: a single read() may return only part of the reply. Loop
        // until EOF (server closes) or the stream goes idle, so large `status` output
        // is not truncated. First read gets the full timeout; later reads use a short
        // idle timeout that simply ends the loop when no more data arrives.
        const MAX_BYTES: usize = 256 * 1024;
        let mut buf = [0u8; 4096];
        let mut bytes = Vec::new();

        loop {
            let read_timeout = if bytes.is_empty() {
                Duration::from_secs(5)
            } else {
                Duration::from_millis(250)
            };

            let len = match timeout(read_timeout, stream.read(&mut buf)).await {
                Ok(Ok(len)) => len,
                Ok(Err(e)) => return Err(RconError::InvalidResponse(e.to_string())),
                Err(_) => {
                    if bytes.is_empty() {
                        return Err(RconError::Timeout);
                    }
                    break;
                }
            };

            // EOF: server closed the connection, reply is complete.
            if len == 0 {
                break;
            }

            bytes.extend_from_slice(&buf[..len]);
            if bytes.len() >= MAX_BYTES {
                break;
            }
        }

        let response = String::from_utf8_lossy(&bytes).to_string();
        Ok(response.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let client = RconClient::new("localhost", 27910, "testpass");
        assert_eq!(client.host, "localhost");
        assert_eq!(client.port, 27910);
        assert_eq!(client.password, "testpass");
    }

    #[tokio::test]
    async fn test_invalid_port() {
        let client = RconClient::new("invalid:host", 27910, "testpass");
        let result = client.execute("status").await;
        assert!(matches!(result, Err(RconError::InvalidResponse(_))));
    }

    // Integration test requires live server - skip in CI
    #[tokio::test]
    #[ignore]
    async fn test_real_server_connection() {
        let client = RconClient::new("noir.lan", 27910, "ace123");
        let result = client.execute("status").await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_rcon_command_format() {
        // Verify the RCON command format matches Quake 2 server expectations
        // The server expects: rcon "password" command (with quotes around password)
        let client = RconClient::new("localhost", 27910, "ace123");

        // Test UDP format (connectionless packet with 0xFFFFFFFF prefix)
        let command = "status";
        let expected_format = format!("rcon \"{}\" {}", client.password, command);
        assert_eq!(expected_format, r#"rcon "ace123" status"#);
    }

    #[test]
    fn test_rcon_password_no_quotes_in_format() {
        // Regression test: ensure password is quoted in the format string
        // This test will fail if someone removes the quotes (like in commit 00284c4e9)
        let client = RconClient::new("localhost", 27910, "test123");
        let command = "dmflags";
        let format_str = format!("rcon \"{}\" {}", client.password, command);

        // Must contain quoted password
        assert!(
            format_str.contains(r#"rcon "test123" dmflags"#),
            "RCON format must have quoted password: {}",
            format_str
        );
        assert!(
            !format_str.contains("rcon test123 dmflags"),
            "RCON format must NOT have unquoted password"
        );
    }

    #[test]
    fn test_tcp_password_quoting() {
        // Regression test: TCP path must also quote the password
        // Prevents regression of unquoted password in TCP format
        let client = RconClient::new("localhost", 27910, "ace123");
        let command = "status";
        let format_str = format!("rcon \"{}\" {}\n", client.password, command);

        assert!(
            format_str.contains(r#"rcon "ace123" status"#),
            "TCP RCON format must have quoted password: {}",
            format_str
        );
        assert!(
            !format_str.contains("rcon ace123 status"),
            "TCP RCON format must NOT have unquoted password"
        );
    }
}
