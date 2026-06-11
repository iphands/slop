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
}

impl RconClient {
    /// Create a new RCON client.
    ///
    /// # Arguments
    /// * `host` - Server hostname.
    /// * `port` - Server RCON port (typically 27910).
    /// * `password` - RCON password.
    pub fn new(host: &str, port: u16, password: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            password: password.to_string(),
        }
    }

    /// Execute an RCON command on the server.
    ///
    /// # Arguments
    /// * `command` - The command to execute (e.g., "status", "dmflags", "kick").
    ///
    /// # Returns
    /// * `Ok(String)` - Server response output.
    /// * `Err(RconError)` - Connection timeout, invalid response, or network error.
    pub async fn execute(&self, command: &str) -> Result<String, RconError> {
        let addr: SocketAddr = format!("{}:{}", self.host, self.port)
            .parse()
            .map_err(|e: std::net::AddrParseError| RconError::InvalidResponse(e.to_string()))?;

        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.connect(addr).await?;

        // Build RCON command: "rcon <password> <command>"
        let rcon_command = format!("rcon {} {}", self.password, command);
        socket.send(rcon_command.as_bytes()).await?;

        // Wait for response with timeout
        let mut buf = [0u8; 4096];
        timeout(Duration::from_secs(5), async {
            socket.recv(&mut buf).await
        })
        .await
        .map_err(|_| RconError::Timeout)?
        .map_err(|e| RconError::InvalidResponse(e.to_string()))?;

        // Parse response - Quake 2 RCON returns the command output
        let response = String::from_utf8_lossy(&buf[..]).to_string();
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
}
