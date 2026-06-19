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

        let mut buf = [0u8; 4096];
        let n = timeout(Duration::from_secs(5), async {
            socket.recv(&mut buf).await
        })
        .await
        .map_err(|_| RconError::Timeout)?
        .map_err(|e| RconError::InvalidResponse(e.to_string()))?;

        // Q2 connectionless packets are prefixed with 0xFFFFFFFF (4 bytes). Only decode
        // the bytes actually received, not the whole zero-padded buffer.
        let payload = buf.get(4..n).unwrap_or(&[]);
        let response = String::from_utf8_lossy(payload).to_string();
        // Strip leading "print\n" if present (added by SV_OobPrintf macro)
        let response = response.strip_prefix("print\n").unwrap_or(&response);
        Ok(response.trim().to_string())
    }

    async fn execute_tcp(&self, addr: SocketAddr, command: &str) -> Result<String, RconError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(addr).await?;

        let rcon_command = format!("rcon \"{}\" {}\n", self.password, command);
        stream.write_all(rcon_command.as_bytes()).await?;

        let mut buf = [0u8; 4096];
        let len = timeout(Duration::from_secs(5), async {
            stream.read(&mut buf).await
        })
        .await
        .map_err(|_| RconError::Timeout)?
        .map_err(|e| RconError::InvalidResponse(e.to_string()))?;

        let response = String::from_utf8_lossy(&buf[..len]).to_string();
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
