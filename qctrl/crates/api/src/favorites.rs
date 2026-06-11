//! Map favorites management module.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Error type for favorites operations.
#[derive(Debug, thiserror::Error)]
pub enum FavoritesError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Thread-safe storage for map favorites.
#[derive(Clone)]
pub struct Favorites {
    storage: Arc<Mutex<FavoritesStorage>>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct FavoritesStorage {
    favorites: Vec<String>, // Map names
}

impl Favorites {
    pub fn new(config_path: &str) -> Result<Self, FavoritesError> {
        let path = Path::new(config_path);
        let storage = if path.exists() {
            let content = fs::read_to_string(path)?;
            serde_json::from_str(&content)?
        } else {
            FavoritesStorage::default()
        };

        Ok(Self {
            storage: Arc::new(Mutex::new(storage)),
        })
    }

    pub fn get_favorites(&self) -> Vec<String> {
        let storage = self.storage.lock().unwrap();
        storage.favorites.clone()
    }

    pub fn add_favorite(&self, map_name: &str) -> Result<(), FavoritesError> {
        let mut storage = self.storage.lock().unwrap();
        if !storage.favorites.contains(&map_name.to_string()) {
            storage.favorites.push(map_name.to_string());
            self.save(&storage)?;
        }
        Ok(())
    }

    pub fn remove_favorite(&self, map_name: &str) -> Result<(), FavoritesError> {
        let mut storage = self.storage.lock().unwrap();
        storage.favorites.retain(|name| name != map_name);
        self.save(&storage)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_favorite(&self, map_name: &str) -> bool {
        let storage = self.storage.lock().unwrap();
        storage.favorites.contains(&map_name.to_string())
    }

    fn save(&self, storage: &FavoritesStorage) -> Result<(), FavoritesError> {
        let path = Path::new("favorites.json");
        let content = serde_json::to_string_pretty(storage)?;
        fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_favorites_basic() {
        let temp_dir = TempDir::new().unwrap();
        let favorites_path = temp_dir.path().join("favorites.json");

        let favorites = Favorites::new(favorites_path.to_str().unwrap()).unwrap();

        assert!(!favorites.is_favorite("q2dm1"));

        favorites.add_favorite("q2dm1").unwrap();
        assert!(favorites.is_favorite("q2dm1"));

        let favs = favorites.get_favorites();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0], "q2dm1");

        favorites.remove_favorite("q2dm1").unwrap();
        assert!(!favorites.is_favorite("q2dm1"));
    }
}
