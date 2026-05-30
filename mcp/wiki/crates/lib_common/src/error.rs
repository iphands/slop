//! Error handling types for the Wikipedia MCP server
//!
//! This module defines error types for all major components using `thiserror`
//! for ergonomic error handling and automatic `std::error::Error` implementation.

use thiserror::Error;

/// Error type for XML parsing operations
#[derive(Error, Debug)]
pub enum WikiParseError {
    #[error("XML parsing error: {0}")]
    XmlError(String),

    #[error("Invalid encoding: {0}")]
    EncodingError(String),

    #[error("Missing required field: {field} in {context}")]
    MissingField { field: String, context: String },

    #[error("Invalid redirect target: {0}")]
    InvalidRedirect(String),

    #[error("Circular redirect detected: {0}")]
    CircularRedirect(String),

    #[error("IO error while reading dump: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<quick_xml::Error> for WikiParseError {
    fn from(err: quick_xml::Error) -> Self {
        WikiParseError::XmlError(err.to_string())
    }
}

/// Error type for chunking operations
#[derive(Error, Debug)]
pub enum ChunkingError {
    #[error("Invalid chunk size: {0} (must be > 0)")]
    InvalidChunkSize(usize),

    #[error("Tokenization failed: {0}")]
    TokenizationError(String),

    #[error("Section parsing error: {0}")]
    SectionParseError(String),

    #[error("Chunk boundary error: {0}")]
    BoundaryError(String),

    #[error("Overlap too large: {0}% exceeds maximum {1}%")]
    OverlapTooLarge(u8, u8),

    #[error("Article too small for chunking: {0} tokens")]
    ArticleTooSmall(usize),
}

/// Error type for embedding generation
#[derive(Error, Debug)]
pub enum EmbeddingError {
    #[error("Model loading failed: {0}")]
    ModelLoadError(String),

    #[error("Inference failed: {0}")]
    InferenceError(String),

    #[error("Invalid embedding dimension: expected {expected}, got {actual}")]
    InvalidDimension { expected: usize, actual: usize },

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Batch processing failed: {0}")]
    BatchError(String),
}

/// Error type for Qdrant vector operations
#[derive(Error, Debug)]
pub enum VectorError {
    #[error("Qdrant connection failed: {0}")]
    ConnectionError(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Collection creation failed: {0}")]
    CollectionCreationError(String),

    #[error("Upsert failed: {0}")]
    UpsertError(String),

    #[error("Search failed: {0}")]
    SearchError(String),

    #[error("Invalid filter: {0}")]
    FilterError(String),

    #[error("Point not found: {0}")]
    PointNotFound(String),
}

/// Error type for MCP server operations
#[derive(Error, Debug)]
pub enum MCPServerError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Tool execution failed: {0}")]
    ToolExecutionError(String),

    #[error("Invalid tool parameters: {0}")]
    InvalidParameters(String),

    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("Server not initialized")]
    NotInitialized,

    #[error("Protocol error: {0}")]
    ProtocolError(String),
}

/// Error type for configuration loading
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    FileNotFound(String),

    #[error("Config parsing failed: {0}")]
    ParseError(String),

    #[error("Missing required config field: {0}")]
    MissingField(String),

    #[error("Invalid config value: {field} = {value} ({reason})")]
    InvalidValue { field: String, value: String, reason: String },

    #[error("Directory not found: {0}")]
    DirectoryNotFound(String),
}

/// Unified error type for the entire system
#[derive(Error, Debug)]
pub enum WikiError {
    #[error("Parse error: {0}")]
    Parse(#[from] WikiParseError),

    #[error("Chunking error: {0}")]
    Chunking(#[from] ChunkingError),

    #[error("Embedding error: {0}")]
    Embedding(#[from] EmbeddingError),

    #[error("Vector error: {0}")]
    Vector(#[from] VectorError),

    #[error("MCP server error: {0}")]
    MCP(#[from] MCPServerError),

    #[error("Config error: {0}")]
    Config(#[from] ConfigError),

    #[error("Generic error: {0}")]
    Generic(String),
}

impl From<String> for WikiError {
    fn from(s: String) -> Self {
        WikiError::Generic(s)
    }
}

impl From<&str> for WikiError {
    fn from(s: &str) -> Self {
        WikiError::Generic(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wiki_parse_error_display() {
        let err = WikiParseError::MissingField {
            field: "title".to_string(),
            context: "page element".to_string(),
        };
        assert!(err.to_string().contains("title"));
        assert!(err.to_string().contains("page element"));
    }

    #[test]
    fn test_chunking_error_display() {
        let err = ChunkingError::InvalidChunkSize(0);
        assert!(err.to_string().contains("0"));
    }

    #[test]
    fn test_embedding_error_display() {
        let err = EmbeddingError::InvalidDimension {
            expected: 384,
            actual: 512,
        };
        assert!(err.to_string().contains("384"));
        assert!(err.to_string().contains("512"));
    }

    #[test]
    fn test_vector_error_display() {
        let err = VectorError::CollectionNotFound("test_collection".to_string());
        assert!(err.to_string().contains("test_collection"));
    }

    #[test]
    fn test_mcp_error_display() {
        let err = MCPServerError::ToolNotFound("wiki_search".to_string());
        assert!(err.to_string().contains("wiki_search"));
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::InvalidValue {
            field: "chunk_size".to_string(),
            value: "-1".to_string(),
            reason: "must be positive".to_string(),
        };
        assert!(err.to_string().contains("chunk_size"));
        assert!(err.to_string().contains("-1"));
    }

    #[test]
    fn test_wiki_error_from_string() {
        let err: WikiError = "test error".to_string().into();
        match err {
            WikiError::Generic(s) => assert_eq!(s, "test error"),
            _ => panic!("Expected Generic error"),
        }
    }

    #[test]
    fn test_wiki_error_from_str() {
        let err: WikiError = "test error".into();
        match err {
            WikiError::Generic(s) => assert_eq!(s, "test error"),
            _ => panic!("Expected Generic error"),
        }
    }
}
