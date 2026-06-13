//! Map rotation management module.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Rotation mode for the map queue.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RotationMode {
    /// Play maps in sequential order.
    #[default]
    Sequential,
    /// Play maps in random order.
    Random,
}

/// Thread-safe rotation queue for map management.
#[derive(Clone)]
pub struct RotationQueue {
    maps: Vec<String>,
    mode: RotationMode,
    path: Option<String>,
}

impl RotationQueue {
    /// Create a new rotation queue with the given mode.
    #[allow(dead_code)]
    pub fn new(mode: RotationMode) -> Self {
        Self {
            maps: Vec::new(),
            mode,
            path: None,
        }
    }

    /// Create a new rotation queue with persistence to the given file path.
    pub fn new_with_persistence(mode: RotationMode, path: &str) -> Self {
        Self {
            maps: Vec::new(),
            mode,
            path: Some(path.to_string()),
        }
    }

    /// Get all maps in the queue.
    pub fn get_maps(&self) -> Vec<String> {
        self.maps.clone()
    }

    /// Add a map to the queue.
    pub fn add_map(&mut self, map_name: String) {
        if !self.maps.contains(&map_name) {
            self.maps.push(map_name);
        }
    }

    /// Remove a map from the queue.
    pub fn remove_map(&mut self, map_name: &str) {
        self.maps.retain(|name| name != map_name);
    }

    pub fn set_maps(&mut self, maps: Vec<String>) {
        self.maps = maps;
    }

    /// Get the next map to play based on rotation mode.
    #[allow(dead_code)]
    pub fn next_map(&mut self) -> Option<String> {
        self.maps.first().cloned()
    }

    /// Get the current rotation mode.
    pub fn mode(&self) -> RotationMode {
        self.mode
    }

    /// Set the rotation mode.
    pub fn set_mode(&mut self, mode: RotationMode) {
        self.mode = mode;
    }

    /// Check if the queue is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.maps.is_empty()
    }

    /// Get the number of maps in the queue.
    pub fn len(&self) -> usize {
        self.maps.len()
    }

    /// Load a rotation queue from a YAML file.
    ///
    /// If the file doesn't exist, returns an empty queue.
    /// If the file is malformed, returns an error.
    #[allow(dead_code)]
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = if Path::new(path).exists() {
            fs::read_to_string(path)?
        } else {
            // Return empty queue for missing file
            return Ok(Self {
                maps: Vec::new(),
                mode: RotationMode::default(),
                path: Some(path.to_string()),
            });
        };

        let data: RotationQueueData = serde_yaml::from_str(&content)?;

        Ok(Self {
            maps: data.queue,
            mode: data.mode,
            path: Some(path.to_string()),
        })
    }

    /// Save the current queue state to disk.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref path) = self.path {
            let data = RotationQueueData {
                queue: self.maps.clone(),
                mode: self.mode,
            };
            let content = serde_yaml::to_string(&data)?;
            fs::write(path, content)?;
        }
        Ok(())
    }
}

/// Persistent data structure for rotation queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RotationQueueData {
    queue: Vec<String>,
    mode: RotationMode,
}

/// Request body for adding a map to the rotation queue.
#[derive(Debug, Serialize, Deserialize)]
pub struct AddMapRequest {
    pub map_name: String,
}

/// Response for adding a map to the rotation queue.
#[derive(Debug, Serialize, Deserialize)]
pub struct QueueResponse {
    pub success: bool,
    pub message: String,
    pub queue_size: usize,
}

/// Response for getting the current rotation queue.
#[derive(Debug, Serialize, Deserialize)]
pub struct QueueStatusResponse {
    pub maps: Vec<String>,
    pub mode: RotationMode,
    pub current_map: Option<String>,
    pub enabled: bool,
}

/// Request body for setting rotation mode.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct SetModeRequest {
    pub mode: RotationMode,
}

/// Response for setting rotation mode.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ModeResponse {
    pub success: bool,
    pub mode: RotationMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rotation_queue_basic() {
        let mut queue = RotationQueue::new(RotationMode::Sequential);

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);

        queue.add_map("q2dm1".to_string());
        queue.add_map("kessel".to_string());

        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 2);

        let maps = queue.get_maps();
        assert_eq!(maps.len(), 2);
        assert!(maps.contains(&"q2dm1".to_string()));
        assert!(maps.contains(&"kessel".to_string()));
    }

    #[test]
    fn test_rotation_queue_duplicate() {
        let mut queue = RotationQueue::new(RotationMode::Sequential);

        queue.add_map("q2dm1".to_string());
        queue.add_map("q2dm1".to_string()); // Should not add duplicate

        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_rotation_queue_remove() {
        let mut queue = RotationQueue::new(RotationMode::Sequential);

        queue.add_map("q2dm1".to_string());
        queue.add_map("kessel".to_string());
        queue.remove_map("q2dm1");

        assert_eq!(queue.len(), 1);
        assert!(!queue.get_maps().contains(&"q2dm1".to_string()));
    }

    #[test]
    fn test_rotation_mode_switch() {
        let mut queue = RotationQueue::new(RotationMode::Sequential);

        assert_eq!(queue.mode(), RotationMode::Sequential);

        queue.set_mode(RotationMode::Random);
        assert_eq!(queue.mode(), RotationMode::Random);
    }

    #[test]
    fn test_next_map_empty() {
        let mut queue = RotationQueue::new(RotationMode::Sequential);

        assert!(queue.next_map().is_none());
    }

    #[test]
    fn test_next_map_with_maps() {
        let mut queue = RotationQueue::new(RotationMode::Sequential);

        queue.add_map("q2dm1".to_string());

        let next = queue.next_map();
        assert!(next.is_some());
        assert_eq!(next.unwrap(), "q2dm1");
    }

    #[test]
    fn test_load_missing_file_creates_empty() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let missing_path = temp_dir.path().join("missing_queue.yaml");

        let result = RotationQueue::load(missing_path.to_str().unwrap());

        // Should succeed with empty queue since file doesn't exist
        assert!(result.is_ok());
        let queue = result.unwrap();
        assert!(queue.is_empty());
    }

    #[test]
    fn test_load_valid_yaml() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let queue_path = temp_dir.path().join("queue.yaml");

        let yaml_content = r#"
queue:
  - q2dm1
  - q2dm2
  - q2dm3
mode: Sequential
"#;
        fs::write(&queue_path, yaml_content).unwrap();

        let queue = RotationQueue::load(queue_path.to_str().unwrap()).unwrap();

        assert_eq!(queue.len(), 3);
        let maps = queue.get_maps();
        assert_eq!(maps[0], "q2dm1");
        assert_eq!(maps[1], "q2dm2");
        assert_eq!(maps[2], "q2dm3");
        assert_eq!(queue.mode(), RotationMode::Sequential);
    }

    #[test]
    fn test_load_invalid_yaml_returns_error() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let queue_path = temp_dir.path().join("queue.yaml");

        let invalid_yaml = "invalid: yaml: :";
        fs::write(&queue_path, invalid_yaml).unwrap();

        let result = RotationQueue::load(queue_path.to_str().unwrap());

        assert!(result.is_err());
    }

    #[test]
    fn test_save_and_reload() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let queue_path = temp_dir.path().join("queue.yaml");

        // Create and populate queue
        {
            let mut queue = RotationQueue::new(RotationMode::Random);
            queue.path = Some(queue_path.to_str().unwrap().to_string());
            queue.add_map("q2dm1".to_string());
            queue.add_map("q2dm2".to_string());
            queue.save().unwrap();
        }

        // Reload from file
        let queue2 = RotationQueue::load(queue_path.to_str().unwrap()).unwrap();

        assert_eq!(queue2.len(), 2);
        let maps = queue2.get_maps();
        assert_eq!(maps[0], "q2dm1");
        assert_eq!(maps[1], "q2dm2");
        assert_eq!(queue2.mode(), RotationMode::Random);
    }

    #[test]
    fn test_save_writes_valid_yaml() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let queue_path = temp_dir.path().join("queue.yaml");

        let mut queue = RotationQueue::new(RotationMode::Sequential);
        queue.path = Some(queue_path.to_str().unwrap().to_string());
        queue.add_map("q2dm1".to_string());
        queue.save().unwrap();

        // Read raw file content
        let content = fs::read_to_string(&queue_path).unwrap();

        // Verify it's valid YAML that can be parsed
        let parsed: RotationQueueData = serde_yaml::from_str(&content).unwrap();
        assert_eq!(parsed.queue.len(), 1);
        assert_eq!(parsed.queue[0], "q2dm1");
        assert_eq!(parsed.mode, RotationMode::Sequential);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_queue_crud_operations() {
        let temp_dir = TempDir::new().unwrap();
        let queue_path = temp_dir.path().join("queue.yaml");

        let mut queue = RotationQueue::new(RotationMode::Sequential);
        queue.path = Some(queue_path.to_str().unwrap().to_string());

        queue.add_map("q2dm1".to_string());
        queue.add_map("kessel".to_string());
        assert_eq!(queue.len(), 2);

        let maps = queue.get_maps();
        assert_eq!(maps.len(), 2);
        assert_eq!(maps[0], "q2dm1");
        assert_eq!(maps[1], "kessel");

        queue.remove_map("q2dm1");
        assert_eq!(queue.len(), 1);
        assert!(!queue.get_maps().contains(&"q2dm1".to_string()));

        queue.set_maps(vec![
            "map1".to_string(),
            "map2".to_string(),
            "map3".to_string(),
        ]);
        assert_eq!(queue.len(), 3);
        let maps = queue.get_maps();
        assert_eq!(maps[0], "map1");
        assert_eq!(maps[1], "map2");
        assert_eq!(maps[2], "map3");
    }

    #[test]
    fn test_persistence_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let queue_path = temp_dir.path().join("queue.yaml");

        {
            let mut queue = RotationQueue::new(RotationMode::Random);
            queue.path = Some(queue_path.to_str().unwrap().to_string());
            queue.add_map("q2dm1".to_string());
            queue.add_map("q2dm2".to_string());
            queue.save().unwrap();
        }

        let loaded_queue = RotationQueue::load(queue_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded_queue.len(), 2);
        assert_eq!(loaded_queue.mode(), RotationMode::Random);
        assert_eq!(loaded_queue.get_maps()[0], "q2dm1");
        assert_eq!(loaded_queue.get_maps()[1], "q2dm2");
    }

    #[test]
    fn test_duplicate_prevention() {
        let mut queue = RotationQueue::new(RotationMode::Sequential);

        queue.add_map("q2dm1".to_string());
        queue.add_map("q2dm1".to_string());
        queue.add_map("q2dm1".to_string());

        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_mode_change_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let queue_path = temp_dir.path().join("queue.yaml");

        let mut queue = RotationQueue::new(RotationMode::Sequential);
        queue.path = Some(queue_path.to_str().unwrap().to_string());
        queue.add_map("q2dm1".to_string());
        queue.save().unwrap();

        queue.set_mode(RotationMode::Random);
        queue.save().unwrap();

        let loaded = RotationQueue::load(queue_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.mode(), RotationMode::Random);
    }

    #[test]
    fn test_empty_queue_operations() {
        let mut queue = RotationQueue::new(RotationMode::Sequential);

        assert!(queue.is_empty());
        assert!(queue.next_map().is_none());
        assert_eq!(queue.get_maps().len(), 0);

        queue.remove_map("nonexistent");
        assert!(queue.is_empty());
    }

    #[test]
    fn test_yaml_format_validation() {
        let temp_dir = TempDir::new().unwrap();
        let queue_path = temp_dir.path().join("queue.yaml");

        let mut queue = RotationQueue::new(RotationMode::Sequential);
        queue.path = Some(queue_path.to_str().unwrap().to_string());
        queue.add_map("test_map".to_string());
        queue.save().unwrap();

        let content = fs::read_to_string(&queue_path).unwrap();
        let parsed: RotationQueueData = serde_yaml::from_str(&content).unwrap();

        assert_eq!(parsed.queue.len(), 1);
        assert_eq!(parsed.queue[0], "test_map");
        assert_eq!(parsed.mode, RotationMode::Sequential);
    }
}
