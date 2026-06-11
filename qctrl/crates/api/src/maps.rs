//! Map listing module for scanning and caching available BSP maps.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Error type for map scanning operations.
#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("Directory not found: {0}")]
    DirectoryNotFound(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Information about a single map file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapInfo {
    pub name: String,
    pub filename: String,
    pub size: u64,
    pub modified: u64,
}

type MapCacheInner = Arc<Mutex<Option<(Vec<MapInfo>, SystemTime)>>>;

/// Thread-safe cache for map listings.
#[derive(Clone)]
pub struct MapCache {
    cache: MapCacheInner,
    baseq2_path: String,
    max_age: Duration,
}

impl MapCache {
    pub fn new(baseq2_path: &str) -> Self {
        Self {
            cache: Arc::new(Mutex::new(None)),
            baseq2_path: baseq2_path.to_string(),
            max_age: Duration::from_secs(300),
        }
    }

    pub fn get_maps(&self) -> Result<Vec<MapInfo>, MapError> {
        let mut cache = self.cache.lock().unwrap();

        if let Some((maps, last_updated)) = cache.as_ref() {
            if last_updated.elapsed().unwrap_or(Duration::MAX) < self.max_age {
                return Ok(maps.clone());
            }
        }

        let maps = self.scan_maps_internal()?;
        *cache = Some((maps.clone(), SystemTime::now()));

        Ok(maps)
    }

    fn scan_maps_internal(&self) -> Result<Vec<MapInfo>, MapError> {
        let maps_dir = Path::new(&self.baseq2_path).join("maps");

        if !maps_dir.exists() {
            return Err(MapError::DirectoryNotFound(maps_dir.to_string_lossy().to_string()));
        }

        let mut maps = Vec::new();

        for entry in fs::read_dir(&maps_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "bsp") {
                let metadata = entry.metadata()?;
                let filename = path.file_name().unwrap().to_string_lossy().to_string();
                let name = filename.trim_end_matches(".bsp").to_string();

                maps.push(MapInfo {
                    name,
                    filename,
                    size: metadata.len(),
                    modified: metadata
                        .modified()
                        .unwrap_or(SystemTime::UNIX_EPOCH)
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO)
                        .as_secs(),
                });
            }
        }

        maps.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(maps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn test_scan_maps_valid_directory() {
        let temp_dir = TempDir::new().unwrap();
        let maps_dir = temp_dir.path().join("maps");
        fs::create_dir(&maps_dir).unwrap();

        // Create test .bsp files
        File::create(maps_dir.join("q2dm1.bsp")).unwrap();
        File::create(maps_dir.join("007_facility.bsp")).unwrap();

        let cache = MapCache::new(temp_dir.path().to_str().unwrap());
        let maps = cache.scan_maps_internal().unwrap();

        assert_eq!(maps.len(), 2);
        assert_eq!(maps[0].name, "007_facility");
        assert_eq!(maps[1].name, "q2dm1");
    }

    #[test]
    fn test_scan_maps_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let maps_dir = temp_dir.path().join("maps");
        fs::create_dir(&maps_dir).unwrap();

        let cache = MapCache::new(temp_dir.path().to_str().unwrap());
        let maps = cache.scan_maps_internal().unwrap();

        assert_eq!(maps.len(), 0);
    }

    #[test]
    fn test_scan_maps_nonexistent_directory() {
        let cache = MapCache::new("/nonexistent/path");
        let result = cache.scan_maps_internal();

        assert!(result.is_err());
    }

    #[test]
    fn test_cache_refresh() {
        let temp_dir = TempDir::new().unwrap();
        let maps_dir = temp_dir.path().join("maps");
        fs::create_dir(&maps_dir).unwrap();
        File::create(maps_dir.join("test.bsp")).unwrap();

        let cache = MapCache::new(temp_dir.path().to_str().unwrap());

        // First call should scan
        let maps1 = cache.get_maps().unwrap();
        assert_eq!(maps1.len(), 1);

        // Second call should use cache
        let maps2 = cache.get_maps().unwrap();
        assert_eq!(maps2.len(), 1);
    }
}
