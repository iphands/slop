//! Map listing module for scanning and caching available BSP maps.
//!
//! Supports scanning both the maps directory and PAK files for .bsp files.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Error type for map scanning operations.
#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("Directory not found: {0}")]
    #[allow(dead_code)]
    DirectoryNotFound(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("PAK file error: {0}")]
    PakError(String),
}

/// Source of a map (directory or PAK file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MapSource {
    Directory,
    Pak(String), // PAK filename
}

/// Information about a single map file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapInfo {
    pub name: String,
    pub filename: String,
    pub size: u64,
    pub modified: u64,
    pub source: MapSource,
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
        let mut maps = Vec::new();
        let baseq2_path = Path::new(&self.baseq2_path);

        // Scan maps directory
        let maps_dir = baseq2_path.join("maps");
        if maps_dir.exists() {
            for entry in fs::read_dir(&maps_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().is_some_and(|ext| ext == "bsp") {
                    let metadata = entry.metadata()?;
                    let filename = path.file_name().unwrap().to_string_lossy().to_string();
                    let name = filename.trim_end_matches(".bsp").to_string();

                    maps.push(MapInfo {
                        name: name.clone(),
                        filename,
                        size: metadata.len(),
                        modified: metadata
                            .modified()
                            .unwrap_or(SystemTime::UNIX_EPOCH)
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or(Duration::ZERO)
                            .as_secs(),
                        source: MapSource::Directory,
                    });
                }
            }
        }

        // Scan PAK files
        let pak_dir = baseq2_path;
        if pak_dir.exists() {
            for entry in fs::read_dir(pak_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().is_some_and(|ext| ext == "pak") {
                    let pak_maps = self.scan_pak_file(&path)?;

                    for pak_map in pak_maps {
                        // Avoid duplicates - if map exists from directory, prefer that
                        if !maps.iter().any(|m| m.name == pak_map.name) {
                            maps.push(pak_map);
                        }
                    }
                }
            }
        }

        // Sort: q2dm* maps first, then alphabetical
        maps.sort_by(|a, b| {
            let a_is_q2dm = a.name.starts_with("q2dm");
            let b_is_q2dm = b.name.starts_with("q2dm");

            match (a_is_q2dm, b_is_q2dm) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        Ok(maps)
    }

    fn scan_pak_file(&self, pak_path: &Path) -> Result<Vec<MapInfo>, MapError> {
        let mut file = fs::File::open(pak_path)
            .map_err(|e| MapError::PakError(format!("Failed to open PAK: {}", e)))?;

        let mut header = [0u8; 1024];
        file.read(&mut header)
            .map_err(|e| MapError::PakError(format!("Failed to read PAK header: {}", e)))?;

        // Check PAK header signature "PACK"
        if &header[0..4] != b"PACK" {
            return Ok(Vec::new());
        }

        let pak_filename = pak_path.file_name().unwrap().to_string_lossy().to_string();
        let mut maps = Vec::new();

        // Parse directory entries (each 64 bytes)
        let dir_offset = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let dir_length = u32::from_le_bytes(header[8..12].try_into().unwrap());

        let num_entries = dir_length / 64;
        if num_entries == 0 {
            return Ok(maps);
        }

        // Seek to directory start and read entries
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(dir_offset as u64))
            .map_err(|e| MapError::PakError(format!("Failed to seek in PAK: {}", e)))?;

        let mut entry_data = vec![0u8; dir_length as usize];
        file.read_exact(&mut entry_data)
            .map_err(|e| MapError::PakError(format!("Failed to read PAK directory: {}", e)))?;

        // Parse each 64-byte entry
        for i in 0..num_entries as usize {
            let offset = i * 64;
            let entry = &entry_data[offset..offset + 64];

            // Filename is first 56 bytes, null-terminated
            let filename_bytes = &entry[0..56];
            let filename = filename_bytes
                .iter()
                .position(|&b| b == 0)
                .map(|pos| &filename_bytes[..pos])
                .unwrap_or(filename_bytes);

            let filename_str = String::from_utf8_lossy(filename).to_lowercase();

            // Check if it's a map file in the maps/ directory
            if filename_str.ends_with(".bsp") && filename_str.starts_with("maps/") {
                let name = filename_str
                    .trim_start_matches("maps/")
                    .trim_end_matches(".bsp");

                maps.push(MapInfo {
                    name: name.to_string(),
                    filename: format!("{}:maps/{}", pak_filename, name),
                    size: 0, // PAK entries don't have easy size access without full parse
                    modified: 0,
                    source: MapSource::Pak(pak_filename.clone()),
                });
            }
        }

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
        // q2dm* maps should be first due to sorting
        assert_eq!(maps[0].name, "q2dm1");
        assert_eq!(maps[1].name, "007_facility");
        assert!(matches!(maps[0].source, MapSource::Directory));
        assert!(matches!(maps[1].source, MapSource::Directory));
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

        // Now returns Ok with empty vec since we scan both maps dir and PAKs
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
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
