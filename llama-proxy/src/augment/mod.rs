//! Augment backend module - experimental feature for enriching requests
//!
//! This module provides functionality to:
//! - Extract user content from requests
//! - Send it to an augment-backend LLM for enrichment
//! - Inject the augmentation back into requests/responses

mod client;
mod extraction;
mod injection;

pub use client::AugmentBackend;
pub use extraction::extract_user_content;
pub use injection::inject_augmentation;

/// API format for augment backend communication
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApiFormat {
    /// OpenAI Chat Completions API format
    OpenAI,
    /// Anthropic Messages API format
    Anthropic,
}

impl ApiFormat {
    /// Auto-detect API format based on URL pattern
    pub fn detect_from_url(url: &str) -> Self {
        let url_lower = url.to_lowercase();
        if url_lower.contains("anthropic") || url_lower.contains("claude") {
            ApiFormat::Anthropic
        } else {
            // Default to OpenAI for llama.cpp and other OpenAI-compatible APIs
            ApiFormat::OpenAI
        }
    }
}
