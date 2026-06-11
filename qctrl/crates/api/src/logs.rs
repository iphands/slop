//! Log streaming module for real-time server log delivery.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};

/// Log entry sent to WebSocket clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: u64,
    pub level: String,
    pub message: String,
}

impl LogEntry {
    pub fn new(level: &str, message: &str) -> Self {
        Self {
            id: 0,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            level: level.to_string(),
            message: message.to_string(),
        }
    }
}

/// Broadcast channel for log entries.
pub type LogSender = broadcast::Sender<LogEntry>;
pub type LogReceiver = broadcast::Receiver<LogEntry>;

/// Log stream manager that broadcasts log entries to subscribers.
pub struct LogStream {
    sender: Arc<LogSender>,
    entry_count: Arc<AtomicU64>,
}

impl LogStream {
    pub fn new(buffer_size: usize) -> Self {
        let (sender, _) = broadcast::channel(buffer_size);
        Self {
            sender: Arc::new(sender),
            entry_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn subscribe(&self) -> LogReceiver {
        self.sender.subscribe()
    }

    pub fn broadcast(&self, level: &str, message: &str) {
        let entry = LogEntry {
            id: self.entry_count.fetch_add(1, Ordering::SeqCst),
            ..LogEntry::new(level, message)
        };
        let _ = self.sender.send(entry);
    }

    /// Simulate log polling from RCON (fallback when file watching not available).
    pub async fn start_polling(&self, rcon_client: Arc<crate::RconClient>) {
        let mut interval = interval(Duration::from_secs(2));
        let mut last_status = String::new();

        loop {
            interval.tick().await;
            
            match rcon_client.execute("status").await {
                Ok(output) => {
                    if output != last_status {
                        if !last_status.is_empty() {
                            self.broadcast("INFO", &format!("Server status changed"));
                        }
                        last_status = output;
                    }
                }
                Err(e) => {
                    self.broadcast("ERROR", &format!("Failed to get status: {}", e));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_creation() {
        let entry = LogEntry::new("INFO", "Test message");
        assert_eq!(entry.level, "INFO");
        assert_eq!(entry.message, "Test message");
        assert!(entry.timestamp > 0);
    }

    #[tokio::test]
    async fn test_log_stream_broadcast() {
        let stream = LogStream::new(100);
        let mut rx = stream.subscribe();

        stream.broadcast("INFO", "Test message");

        let entry = rx.recv().await.unwrap();
        assert_eq!(entry.level, "INFO");
        assert_eq!(entry.message, "Test message");
    }
}
