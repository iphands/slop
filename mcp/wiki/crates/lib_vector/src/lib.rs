//! Qdrant client wrapper for Wikipedia MCP server.
//!
//! Provides vector storage with support for dense vectors and metadata filtering.

use anyhow::{Context, Result};
use qdrant_client::{qdrant, Qdrant};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for Qdrant connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QdrantConfig {
    /// Qdrant host address
    pub host: String,
    /// Qdrant port
    pub port: u16,
    /// Use TLS
    pub tls: bool,
    /// API key (optional)
    pub api_key: Option<String>,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 6333,
            tls: false,
            api_key: None,
        }
    }
}

impl QdrantConfig {
    /// Create client from config
    pub fn create_client(&self) -> Result<Qdrant> {
        let url = if self.tls {
            format!("https://{}:{}", self.host, self.port)
        } else {
            format!("http://{}:{}", self.host, self.port)
        };

        let client = Qdrant::from_url(&url)
            .build()
            .context("Failed to build Qdrant client")?;

        Ok(client)
    }
}

/// Wikipedia chunk metadata for storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Article title
    pub title: String,
    /// Section path (e.g., "History/Early Period")
    pub section_path: Option<String>,
    /// Chunk text
    pub text: String,
    /// Character offset in original article
    pub offset: Option<u32>,
    /// Namespace (0 for articles, 1 for talk pages, etc.)
    pub namespace: i32,
}

impl ChunkMetadata {
    /// Convert to Qdrant payload
    pub fn to_payload(&self) -> HashMap<String, qdrant::Value> {
        let mut payload = HashMap::new();

        payload.insert("title".to_string(), qdrant::Value::from(self.title.clone()));

        if let Some(section) = &self.section_path {
            payload.insert("section_path".to_string(), qdrant::Value::from(section.clone()));
        }

        payload.insert("text".to_string(), qdrant::Value::from(self.text.clone()));

        if let Some(offset) = self.offset {
            payload.insert("offset".to_string(), qdrant::Value::from(offset as i64));
        }

        payload.insert("namespace".to_string(), qdrant::Value::from(self.namespace as i64));

        payload
    }
}

/// Vector store wrapper
pub struct VectorStore {
    client: Qdrant,
    collection_name: String,
    vector_size: usize,
}

impl VectorStore {
    /// Create new vector store connection
    pub fn new(client: Qdrant, collection_name: String, vector_size: usize) -> Self {
        Self {
            client,
            collection_name,
            vector_size,
        }
    }

    /// Create collection with vector storage
    pub async fn create_collection(&self) -> Result<()> {
        use qdrant_client::qdrant::CreateCollectionBuilder;
        use qdrant_client::qdrant::VectorParamsBuilder;

        let vector_params = VectorParamsBuilder::new(
            self.vector_size as u64,
            qdrant::Distance::Cosine,
        );

        self.client
            .create_collection(
                CreateCollectionBuilder::new(&self.collection_name)
                    .vectors_config(vector_params)
            )
            .await
            .context("Failed to create collection")?;

        Ok(())
    }

    /// Upsert points to collection
    pub async fn upsert(
        &self,
        points: Vec<qdrant::PointStruct>,
    ) -> Result<()> {
        use qdrant_client::qdrant::UpsertPointsBuilder;

        self.client
            .upsert_points(
                UpsertPointsBuilder::new(&self.collection_name, points)
                    .wait(true)
            )
            .await
            .context("Failed to upsert points")?;

        Ok(())
    }

    /// Search with dense vector
    pub async fn search_dense(
        &self,
        vector: Vec<f32>,
        limit: u64,
        filter: Option<qdrant::Filter>,
    ) -> Result<Vec<qdrant::ScoredPoint>> {
        use qdrant_client::qdrant::SearchPointsBuilder;

        let mut builder = SearchPointsBuilder::new(
            &self.collection_name,
            vector,
            limit,
        )
        .score_threshold(0.0)
        .with_payload(true);

        if let Some(f) = filter {
            builder = builder.filter(f);
        }

        let result = self.client
            .search_points(builder)
            .await
            .context("Failed to search collection")?;

        Ok(result.result)
    }

    /// Build metadata filter from optional criteria
    pub fn build_filter(
        title: Option<&str>,
        namespace: Option<i32>,
    ) -> Option<qdrant::Filter> {
        let mut must = Vec::new();

        if let Some(t) = title {
            must.push(qdrant::Condition::matches("title", t.to_string()));
        }

        if let Some(ns) = namespace {
            must.push(qdrant::Condition::matches("namespace", ns as i64));
        }

        if must.is_empty() {
            None
        } else {
            Some(qdrant::Filter::must(must))
        }
    }

    /// Optimize collection for local deployment
    pub async fn optimize_local(&self) -> Result<()> {
        use qdrant_client::qdrant::UpdateCollectionBuilder;
        use qdrant_client::qdrant::HnswConfigDiff;

        let hnsw_config = HnswConfigDiff {
            m: Some(16),
            ef_construct: Some(200),
            full_scan_threshold: Some(10000),
            ..Default::default()
        };

        self.client
            .update_collection(
                UpdateCollectionBuilder::new(&self.collection_name)
                    .hnsw_config(hnsw_config)
            )
            .await
            .context("Failed to optimize collection")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qdrant_config_default() {
        let config = QdrantConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 6333);
        assert!(!config.tls);
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_chunk_metadata_payload() {
        let metadata = ChunkMetadata {
            title: "Test Article".to_string(),
            section_path: Some("History".to_string()),
            text: "Test text".to_string(),
            offset: Some(100),
            namespace: 0,
        };

        let payload = metadata.to_payload();
        assert!(payload.contains_key("title"));
        assert!(payload.contains_key("section_path"));
        assert!(payload.contains_key("text"));
        assert!(payload.contains_key("offset"));
        assert!(payload.contains_key("namespace"));
    }

    #[test]
    fn test_filter_building() {
        // No filters
        let filter = VectorStore::build_filter(None, None);
        assert!(filter.is_none());

        // Title filter
        let filter = VectorStore::build_filter(Some("Test"), None);
        assert!(filter.is_some());

        // Namespace filter
        let filter = VectorStore::build_filter(None, Some(0));
        assert!(filter.is_some());

        // Both filters
        let filter = VectorStore::build_filter(Some("Test"), Some(0));
        assert!(filter.is_some());
    }
}
