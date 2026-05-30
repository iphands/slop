//! Shared types and utilities for Wikipedia MCP server
//!
//! This crate defines the core domain types used throughout the system:
//! - `Article`: Represents a Wikipedia article
//! - `Chunk`: Represents a chunked section of an article
//! - `SearchParams`: Configuration for search queries
//! - `SearchResult`: Results from search operations
//! - `Embedding`: Vector embeddings for semantic search
//! - `WikiConfig`: Configuration for the ingestion pipeline
//!
//! Also provides error types in the `error` module.

pub mod error;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export error types for convenience
pub use error::*;

/// Core representation of a Wikipedia article
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Article {
    /// Unique identifier for the article
    pub id: String,
    
    /// Article title (e.g., "Main_Page")
    pub title: String,
    
    /// Raw article text (may contain MediaWiki markup)
    pub text: String,
    
    /// Cleaned article text (markup removed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleaned_text: Option<String>,
    
    /// Namespace ID (0 = article, 1 = talk, etc.)
    pub namespace: i32,
    
    /// If this is a redirect, the target article title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirects_to: Option<String>,
    
    /// Last modification timestamp
    pub timestamp: DateTime<Utc>,
    
    /// Article creation timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

impl Article {
    /// Create a new article
    pub fn new(id: String, title: String, text: String, namespace: i32, timestamp: DateTime<Utc>) -> Self {
        Self {
            id,
            title,
            text,
            cleaned_text: None,
            namespace,
            redirects_to: None,
            timestamp,
            created_at: None,
        }
    }

    /// Check if this article is a redirect
    pub fn is_redirect(&self) -> bool {
        self.redirects_to.is_some()
    }

    /// Get the cleaned text, falling back to raw text if not cleaned
    pub fn get_text(&self) -> &str {
        self.cleaned_text.as_deref().unwrap_or(&self.text)
    }
}

/// A chunked section of an article for embedding and search
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chunk {
    /// Unique identifier for this chunk
    pub id: String,
    
    /// ID of the source article
    pub article_id: String,
    
    /// Title of the source article
    pub article_title: String,
    
    /// Index of this chunk within the article (0-based)
    pub chunk_index: usize,
    
    /// Section path (e.g., "History/Early Period") if applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_path: Option<String>,
    
    /// The chunk text content
    pub text: String,
    
    /// Number of tokens in this chunk
    pub token_count: usize,
    
    /// Character offset within the article (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    
    /// Length of the chunk in characters
    pub length: usize,
}

impl Chunk {
    /// Create a new chunk
    pub fn new(id: String, article_id: String, article_title: String, chunk_index: usize, text: String, token_count: usize) -> Self {
        let length = text.len();
        Self {
            id,
            article_id,
            article_title,
            chunk_index,
            section_path: None,
            text,
            token_count,
            offset: None,
            length,
        }
    }

    /// Set the section path for this chunk
    pub fn with_section_path(mut self, path: String) -> Self {
        self.section_path = Some(path);
        self
    }

    /// Set the offset for this chunk
    pub fn with_offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }
}

/// Configuration parameters for search queries
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchParams {
    /// Search query string
    pub query: String,
    
    /// Maximum number of results to return
    pub limit: usize,
    
    /// Whether to include embeddings in results
    #[serde(default)]
    pub include_embeddings: bool,
    
    /// Filter by namespace (None = all namespaces)
    /// Wikipedia namespaces: 0=article, 1=talk, 2=user, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace_filter: Option<i32>,
    
    /// Minimum relevance score threshold (0.0 to 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_threshold: Option<f32>,
}

impl SearchParams {
    /// Create new search params with defaults
    pub fn new(query: String) -> Self {
        Self {
            query,
            limit: 10,
            include_embeddings: false,
            namespace_filter: Some(0), // Default to articles only
            score_threshold: None,
        }
    }

    /// Set the result limit
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Set the namespace filter
    pub fn with_namespace(mut self, namespace: i32) -> Self {
        self.namespace_filter = Some(namespace);
        self
    }

    /// Set the score threshold (must be between 0.0 and 1.0)
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        assert!(threshold >= 0.0 && threshold <= 1.0, "threshold must be between 0.0 and 1.0");
        self.score_threshold = Some(threshold);
        self
    }
}

/// A search result with scoring information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    /// The matched chunk
    pub chunk: Chunk,
    
    /// Relevance score (0.0 to 1.0)
    pub score: f32,
    
    /// Highlighted snippet of matching text (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    
    /// Embedding vector (optional, for debugging)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

impl SearchResult {
    /// Create a new search result
    pub fn new(chunk: Chunk, score: f32) -> Self {
        Self {
            chunk,
            score,
            highlight: None,
            embedding: None,
        }
    }

    /// Set the highlight snippet
    pub fn with_highlight(mut self, highlight: String) -> Self {
        self.highlight = Some(highlight);
        self
    }

    /// Set the embedding
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }
}

/// A vector embedding for semantic search
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Embedding {
    /// Source text that was embedded
    pub text: String,
    
    /// The embedding vector
    pub vector: Vec<f32>,
    
    /// Model used to generate the embedding
    pub model: String,
}

impl Embedding {
    /// Create a new embedding
    pub fn new(text: String, vector: Vec<f32>, model: String) -> Self {
        Self { text, vector, model }
    }

    /// Get the dimension of the embedding
    pub fn dimension(&self) -> usize {
        self.vector.len()
    }
}

/// Configuration for the ingestion pipeline
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WikiConfig {
    /// Directory containing Wikipedia dump files
    pub dump_dir: PathBuf,
    
    /// Directory for generated artifacts
    pub output_dir: PathBuf,
    
    /// Minimum chunk size in tokens
    #[serde(default = "default_chunk_min")]
    pub chunk_size_min: usize,
    
    /// Maximum chunk size in tokens
    #[serde(default = "default_chunk_max")]
    pub chunk_size_max: usize,
    
    /// Overlap between chunks as a fraction (0.0 to 1.0)
    #[serde(default = "default_overlap")]
    pub overlap: f64,
    
    /// Embedding model name
    #[serde(default = "default_model")]
    pub embedding_model: String,
    
    /// Qdrant host
    #[serde(default = "default_qdrant_host")]
    pub qdrant_host: String,
    
    /// Qdrant port
    #[serde(default = "default_qdrant_port")]
    pub qdrant_port: u16,
    
    /// Collection name in Qdrant
    #[serde(default = "default_collection")]
    pub collection_name: String,
}

fn default_chunk_min() -> usize { 300 }
fn default_chunk_max() -> usize { 800 }
fn default_overlap() -> f64 { 0.15 }
fn default_model() -> String { "bge-small-en-v1.5".to_string() }
fn default_qdrant_host() -> String { "localhost".to_string() }
fn default_qdrant_port() -> u16 { 6333 }
fn default_collection() -> String { "wikipedia_chunks".to_string() }

impl Default for WikiConfig {
    fn default() -> Self {
        Self {
            dump_dir: PathBuf::from("ai/data/wikipedia"),
            output_dir: PathBuf::from("scratch/ingest"),
            chunk_size_min: default_chunk_min(),
            chunk_size_max: default_chunk_max(),
            overlap: default_overlap(),
            embedding_model: default_model(),
            qdrant_host: default_qdrant_host(),
            qdrant_port: default_qdrant_port(),
            collection_name: default_collection(),
        }
    }
}

impl WikiConfig {
    /// Create a new config with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the dump directory
    pub fn with_dump_dir(mut self, dir: PathBuf) -> Self {
        self.dump_dir = dir;
        self
    }

    /// Set the output directory
    pub fn with_output_dir(mut self, dir: PathBuf) -> Self {
        self.output_dir = dir;
        self
    }

    /// Set chunk size range
    pub fn with_chunk_size(mut self, min: usize, max: usize) -> Self {
        self.chunk_size_min = min;
        self.chunk_size_max = max;
        self
    }

    /// Set Qdrant connection
    pub fn with_qdrant(mut self, host: String, port: u16) -> Self {
        self.qdrant_host = host;
        self.qdrant_port = port;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_article_creation() {
        let article = Article::new(
            "1".to_string(),
            "Test_Article".to_string(),
            "Test content".to_string(),
            0,
            Utc::now(),
        );
        
        assert_eq!(article.id, "1");
        assert_eq!(article.title, "Test_Article");
        assert!(!article.is_redirect());
    }

    #[test]
    fn test_chunk_creation() {
        let chunk = Chunk::new(
            "chunk-1".to_string(),
            "1".to_string(),
            "Test_Article".to_string(),
            0,
            "Chunk text".to_string(),
            100,
        );
        
        assert_eq!(chunk.token_count, 100);
        assert_eq!(chunk.length, 10);
    }

    #[test]
    fn test_search_params_defaults() {
        let params = SearchParams::new("test query".to_string());
        
        assert_eq!(params.query, "test query");
        assert_eq!(params.limit, 10);
        assert_eq!(params.namespace_filter, Some(0));
    }

    #[test]
    fn test_config_defaults() {
        let config = WikiConfig::default();
        
        assert_eq!(config.chunk_size_min, 300);
        assert_eq!(config.chunk_size_max, 800);
        assert_eq!(config.overlap, 0.15);
        assert_eq!(config.qdrant_port, 6333);
    }
}
