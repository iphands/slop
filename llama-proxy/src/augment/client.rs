//! Augment backend client

use reqwest::Client;
use std::time::Duration;

use super::ApiFormat;
use crate::config::AugmentBackendConfig;

/// Client for communicating with augment backend
pub struct AugmentBackend {
    url: String,
    model: String,
    prompt_file: String,
    request_prompt_file: String,
    http_client: Client,
    api_format: ApiFormat,
}

impl AugmentBackend {
    /// Create a new augment backend client from config
    pub fn from_config(config: &AugmentBackendConfig) -> Result<Self, crate::config::AugmentBackendError> {
        if config.url.is_empty() {
            return Err(crate::config::AugmentBackendError {
                message: "Augment backend URL is empty".to_string(),
            });
        }

        if config.model.is_empty() {
            return Err(crate::config::AugmentBackendError {
                message: "Augment backend model is empty".to_string(),
            });
        }

        let http_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| crate::config::AugmentBackendError {
                message: format!("Failed to create HTTP client: {}", e),
            })?;

        Ok(Self {
            url: config.url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            prompt_file: config.prompt_file.clone(),
            request_prompt_file: config.request_prompt_file.clone(),
            http_client,
            api_format: ApiFormat::detect_from_url(&config.url),
        })
    }

    /// Load the prompt from the prompt file
    pub fn load_prompt(&self) -> Result<String, crate::config::AugmentBackendError> {
        std::fs::read_to_string(&self.prompt_file)
            .map_err(|e| crate::config::AugmentBackendError {
                message: format!("Failed to read prompt file '{}': {}", self.prompt_file, e),
            })
    }

    /// Load the request prompt (injected into enriched user message)
    pub fn load_request_prompt(&self) -> Result<String, crate::config::AugmentBackendError> {
        std::fs::read_to_string(&self.request_prompt_file)
            .map_err(|e| crate::config::AugmentBackendError {
                message: format!("Failed to read request_prompt file '{}': {}", self.request_prompt_file, e),
            })
    }

    /// Get augmentation for user content
    pub async fn get_augmentation(
        &self,
        user_content: &str,
    ) -> Result<String, crate::config::AugmentBackendError> {
        // Load prompt
        let prompt = self.load_prompt()?;

        // Combine prompt and user content
        let full_prompt = format!("{}{}", prompt, user_content);

        // Send to augment backend based on API format
        let augmentation = match self.api_format {
            ApiFormat::OpenAI => self.send_openai_request(&full_prompt).await?,
            ApiFormat::Anthropic => self.send_anthropic_request(&full_prompt).await?,
        };

        Ok(augmentation)
    }

    /// Send request to augment backend using OpenAI format
    async fn send_openai_request(
        &self,
        prompt: &str,
    ) -> Result<String, crate::config::AugmentBackendError> {
        let request = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "stream": false,
        });

        let url = format!("{}/v1/chat/completions", self.url);
        tracing::debug!(url = %url, "Sending request to augment backend (OpenAI format)");

        let response = self.http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| crate::config::AugmentBackendError {
                message: format!("Failed to send request to augment backend: {}", e),
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(crate::config::AugmentBackendError {
                message: format!("Augment backend returned error {}: {}", status, error_body),
            });
        }

        let response_json: serde_json::Value = response.json().await
            .map_err(|e| crate::config::AugmentBackendError {
                message: format!("Failed to parse augment backend response: {}", e),
            })?;

        // Extract augmentation text from response
        let augmentation = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|content| content.as_str())
            .unwrap_or("")
            .to_string();

        if augmentation.is_empty() {
            tracing::warn!("Augment backend returned empty augmentation");
        }

        Ok(augmentation)
    }

    /// Send request to augment backend using Anthropic format
    async fn send_anthropic_request(
        &self,
        prompt: &str,
    ) -> Result<String, crate::config::AugmentBackendError> {
        let request = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": prompt
                        }
                    ]
                }
            ],
            "max_tokens": 4096,
        });

        let url = format!("{}/v1/messages", self.url);
        tracing::debug!(url = %url, "Sending request to augment backend (Anthropic format)");

        let response = self.http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| crate::config::AugmentBackendError {
                message: format!("Failed to send request to augment backend: {}", e),
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(crate::config::AugmentBackendError {
                message: format!("Augment backend returned error {}: {}", status, error_body),
            });
        }

        let response_json: serde_json::Value = response.json().await
            .map_err(|e| crate::config::AugmentBackendError {
                message: format!("Failed to parse augment backend response: {}", e),
            })?;

        // Extract augmentation text from response
        let augmentation = response_json
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|block| block.get("text"))
            .and_then(|text| text.as_str())
            .unwrap_or("")
            .to_string();

        if augmentation.is_empty() {
            tracing::warn!("Augment backend returned empty augmentation");
        }

        Ok(augmentation)
    }
}
