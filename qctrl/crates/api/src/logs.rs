//! Log streaming module for real-time server log delivery via WebSocket.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

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

/// Log stream manager that broadcasts log entries to subscribers with history.
pub struct LogStream {
    sender: Arc<LogSender>,
    entry_count: Arc<AtomicU64>,
    history: Arc<Mutex<VecDeque<LogEntry>>>,
    max_history: usize,
}

impl LogStream {
    pub fn new(buffer_size: usize, max_history: usize) -> Self {
        let (sender, _) = broadcast::channel(buffer_size);
        Self {
            sender: Arc::new(sender),
            entry_count: Arc::new(AtomicU64::new(0)),
            history: Arc::new(Mutex::new(VecDeque::with_capacity(max_history))),
            max_history,
        }
    }

    pub fn subscribe(&self) -> (LogReceiver, Vec<LogEntry>) {
        let rx = self.sender.subscribe();
        let history = self.history.lock().unwrap().iter().cloned().collect();
        (rx, history)
    }

    pub fn broadcast(&self, level: &str, message: &str) {
        let entry = LogEntry {
            id: self.entry_count.fetch_add(1, Ordering::SeqCst),
            ..LogEntry::new(level, message)
        };
        let entry_for_log = entry.clone();

        // Store in history
        {
            let mut history = self.history.lock().unwrap();
            history.push_back(entry.clone());
            if history.len() > self.max_history {
                history.pop_front();
            }
        }

        // Broadcast to subscribers (may fail if no subscribers, which is OK)
        let _ = self.sender.send(entry);
        tracing::debug!("Broadcast log entry: {:?}", entry_for_log);
    }

    /// Unused: `subscribe` already hands back the history alongside the receiver,
    /// which is what every caller actually needs. Kept as a plain accessor.
    #[allow(dead_code)]
    pub fn get_history(&self) -> Vec<LogEntry> {
        self.history.lock().unwrap().iter().cloned().collect()
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
        let _stream = LogStream::new(100, 50);
        let (sender, mut rx) = broadcast::channel(100);
        let stream_for_test = LogStream {
            sender: Arc::new(sender),
            entry_count: Arc::new(AtomicU64::new(0)),
            history: Arc::new(Mutex::new(VecDeque::new())),
            max_history: 50,
        };

        stream_for_test.broadcast("INFO", "Test message");

        let entry = rx.recv().await.unwrap();
        assert_eq!(entry.level, "INFO");
        assert_eq!(entry.message, "Test message");
    }
}
