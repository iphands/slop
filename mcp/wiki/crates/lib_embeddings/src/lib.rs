//! Embedding generation interface for Wikipedia MCP server.
//!
//! Provides trait-based abstraction for embedding generation with
//! FastEmbed backend for offline, CPU-optimized inference.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Configuration for embedding model selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Model to use (default: BGESmallEnV15 for best balance)
    pub model: EmbeddingModel,
    /// Cache directory for downloaded models
    pub cache_dir: Option<String>,
    /// Show download progress
    pub show_progress: bool,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: EmbeddingModel::BGESmallEnV15,
            cache_dir: None,
            show_progress: true,
        }
    }
}

/// Supported embedding models
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum EmbeddingModel {
    /// BGE Small EN v1.5 (quantized) - 45MB, 384 dims, best balance
    BGESmallEnV15,
    /// All MiniLM L6 v2 - 45MB, 384 dims, smaller footprint
    AllMiniLML6V2,
    /// All MPNet Base v2 - 170MB, 768 dims, higher accuracy
    AllMPNetBaseV2,
    /// BGE Base EN v1.5 - 140MB, 768 dims, higher accuracy
    BGEBaseEnV15,
}

impl fmt::Display for EmbeddingModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmbeddingModel::BGESmallEnV15 => write!(f, "bge-small-en-v1.5-onnx-q"),
            EmbeddingModel::AllMiniLML6V2 => write!(f, "all-MiniLM-L6-v2"),
            EmbeddingModel::AllMPNetBaseV2 => write!(f, "all-mpnet-base-v2"),
            EmbeddingModel::BGEBaseEnV15 => write!(f, "bge-base-en-v1.5"),
        }
    }
}

impl EmbeddingModel {
    /// Get the underlying fastembed model variant
    pub fn to_fastembed_model(&self) -> fastembed::EmbeddingModel {
        match self {
            EmbeddingModel::BGESmallEnV15 => fastembed::EmbeddingModel::BGESmallENV15,
            EmbeddingModel::AllMiniLML6V2 => fastembed::EmbeddingModel::AllMiniLML6V2,
            EmbeddingModel::AllMPNetBaseV2 => fastembed::EmbeddingModel::ParaphraseMLMpnetBaseV2,
            EmbeddingModel::BGEBaseEnV15 => fastembed::EmbeddingModel::BGEBaseENV15,
        }
    }

    /// Get expected vector dimension for this model
    pub fn dimension(&self) -> usize {
        match self {
            EmbeddingModel::BGESmallEnV15 | EmbeddingModel::AllMiniLML6V2 => 384,
            EmbeddingModel::AllMPNetBaseV2 | EmbeddingModel::BGEBaseEnV15 => 768,
        }
    }
}

/// Generated embedding with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    /// Text that was embedded
    pub text: String,
    /// Embedding vector
    pub vector: Vec<f32>,
    /// Model used for generation
    pub model: String,
}

/// Interface for embedding generation
#[async_trait::async_trait]
pub trait EmbeddingGenerator: Send + Sync {
    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> Result<Embedding>;

    /// Generate embeddings for multiple texts (batch)
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>>;

    /// Get the expected dimension of embeddings
    fn dimension(&self) -> usize;

    /// Get the model name
    fn model_name(&self) -> &str;
}

/// FastEmbed-based implementation
pub struct FastEmbedGenerator {
    model: fastembed::TextEmbedding,
    config: EmbeddingConfig,
}

impl FastEmbedGenerator {
    /// Create new generator with default config
    pub fn new() -> Result<Self> {
        Self::with_config(EmbeddingConfig::default())
    }

    /// Create with custom config
    pub fn with_config(config: EmbeddingConfig) -> Result<Self> {
        use fastembed::InitOptions;
        use std::path::PathBuf;
        
        let cache_path = config.cache_dir.clone().map(PathBuf::from);
        let init_options = InitOptions::new(config.model.to_fastembed_model())
            .with_show_download_progress(config.show_progress);
        
        let init_options = if let Some(cache) = cache_path {
            init_options.with_cache_dir(cache)
        } else {
            init_options
        };
        
        let model = fastembed::TextEmbedding::try_new(init_options)
            .context("Failed to initialize FastEmbed model")?;

        Ok(Self { model, config })
    }

    /// Convert fastembed output to our Embedding type
    fn to_embedding(&self, text: &str, vector: &[f32]) -> Embedding {
        Embedding {
            text: text.to_string(),
            vector: vector.to_vec(),
            model: self.config.model.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl EmbeddingGenerator for FastEmbedGenerator {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        let result = self.model.embed(vec![text.to_string()], None)?;
        let vector = result
            .first()
            .context("No embedding returned")?;

        Ok(self.to_embedding(text, vector))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Embedding>> {
        let results = self.model.embed(texts.to_vec(), None)?;

        Ok(texts
            .iter()
            .zip(results.iter())
            .map(|(text, vector)| self.to_embedding(text, vector))
            .collect())
    }

    fn dimension(&self) -> usize {
        self.config.model.dimension()
    }

    fn model_name(&self) -> &str {
        match self.config.model {
            EmbeddingModel::BGESmallEnV15 => "bge-small-en-v1.5-onnx-q",
            EmbeddingModel::AllMiniLML6V2 => "all-MiniLM-L6-v2",
            EmbeddingModel::AllMPNetBaseV2 => "all-mpnet-base-v2",
            EmbeddingModel::BGEBaseEnV15 => "bge-base-en-v1.5",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_dimension() {
        assert_eq!(EmbeddingModel::BGESmallEnV15.dimension(), 384);
        assert_eq!(EmbeddingModel::AllMiniLML6V2.dimension(), 384);
        assert_eq!(EmbeddingModel::AllMPNetBaseV2.dimension(), 768);
        assert_eq!(EmbeddingModel::BGEBaseEnV15.dimension(), 768);
    }

    #[tokio::test]
    async fn test_embedding_generation() {
        let config = EmbeddingConfig {
            model: EmbeddingModel::AllMiniLML6V2,
            cache_dir: Some("/tmp/test_embeddings".to_string()),
            show_progress: false,
        };

        let generator = FastEmbedGenerator::with_config(config);
        assert!(generator.is_ok());

        let generator = generator.unwrap();
        let embedding = generator.embed("Hello, world!").await;
        assert!(embedding.is_ok());

        let embedding = embedding.unwrap();
        assert_eq!(embedding.text, "Hello, world!");
        assert_eq!(embedding.vector.len(), 384);
    }

    #[tokio::test]
    async fn test_batch_embedding() {
        let generator = FastEmbedGenerator::new().unwrap();
        let texts = vec![
            "First text".to_string(),
            "Second text".to_string(),
            "Third text".to_string(),
        ];

        let embeddings = generator.embed_batch(&texts).await.unwrap();
        assert_eq!(embeddings.len(), 3);
        assert_eq!(embeddings[0].vector.len(), 384);
    }
}
