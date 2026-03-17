//! Inject augmentation into request messages

use crate::api::{ChatCompletionRequest, Message, MessageContent};

/// Inject augmentation into the last user message
///
/// The augmentation is appended to the user message content in the format:
/// `original_content\n\n<augmentation>\n\n<augmentation_result>`
///
/// If no user message is found, the augmentation is prepended to the first message.
pub fn inject_augmentation(
    mut request: ChatCompletionRequest,
    augmentation: &str,
) -> Result<ChatCompletionRequest, crate::config::AugmentBackendError> {
    if augmentation.is_empty() {
        tracing::debug!("Empty augmentation, returning request unchanged");
        return Ok(request);
    }

    // Find the last user message
    let mut last_user_index = None;
    for (i, msg) in request.messages.iter().enumerate() {
        if msg.role == "user" {
            last_user_index = Some(i);
        }
    }

    if let Some(idx) = last_user_index {
        // Inject into last user message
        let augmentation_with_markers = format!("\n\n<augmentation>\n{}\n</augmentation>\n\n", augmentation);

        request.messages[idx] = modify_message_content(
            &request.messages[idx],
            &augmentation_with_markers,
        );

        tracing::debug!(
            injected_into = idx,
            augmentation_length = augmentation.len(),
            "Injected augmentation into last user message"
        );
    } else if !request.messages.is_empty() {
        // Fallback: prepend to first message if no user message found
        let augmentation_marker = format!("<augmentation>\n{}\n</augmentation>\n\n", augmentation);

        request.messages[0] = modify_message_content(
            &request.messages[0],
            &augmentation_marker,
        );

        tracing::debug!(
            injected_into = 0,
            augmentation_length = augmentation.len(),
            "Injected augmentation into first message (no user message found)"
        );
    } else {
        return Err(crate::config::AugmentBackendError {
            message: "No messages in request to inject augmentation into".to_string(),
        });
    }

    Ok(request)
}

/// Modify message content by appending augmentation
fn modify_message_content(msg: &Message, augmentation: &str) -> Message {
    let mut new_msg = msg.clone();

    match &msg.content {
        Some(MessageContent::Text(text)) => {
            new_msg.content = Some(MessageContent::Text(format!("{}{}", text, augmentation)));
        }
        Some(MessageContent::Parts(parts)) => {
            // For multimodal messages, add text part at the end
            let mut new_parts = parts.clone();
            new_parts.push(crate::api::ContentPart {
                content_type: "text".to_string(),
                text: Some(augmentation.to_string()),
                image_url: None,
            });
            new_msg.content = Some(MessageContent::Parts(new_parts));
        }
        None => {
            new_msg.content = Some(MessageContent::Text(augmentation.to_string()));
        }
    }

    new_msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_augmentation_simple() {
        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: Some(MessageContent::Text("You are helpful".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    role: "user".to_string(),
                    content: Some(MessageContent::Text("Hello".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            user: None,
            reasoning_effort: None,
            verbosity: None,
            thinking_budget: None,
        };

        let result = inject_augmentation(request, "Context info").unwrap();

        assert_eq!(result.messages.len(), 2);
        let user_msg = &result.messages[1];
        if let Some(MessageContent::Text(text)) = &user_msg.content {
            assert!(text.contains("Hello"));
            assert!(text.contains("<augmentation>"));
            assert!(text.contains("Context info"));
        } else {
            panic!("Expected text content");
        }
    }

    #[test]
    fn test_inject_augmentation_empty() {
        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: Some(MessageContent::Text("Hello".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            user: None,
            reasoning_effort: None,
            verbosity: None,
            thinking_budget: None,
        };

        let result = inject_augmentation(request, "").unwrap();
        // Empty augmentation should leave request unchanged
        if let Some(MessageContent::Text(text)) = &result.messages[0].content {
            assert_eq!(text, "Hello");
        }
    }

    #[test]
    fn test_inject_augmentation_no_user() {
        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: Some(MessageContent::Text("System prompt".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            tools: None,
            tool_choice: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            user: None,
            reasoning_effort: None,
            verbosity: None,
            thinking_budget: None,
        };

        let result = inject_augmentation(request, "Context").unwrap();
        // Should inject into first message when no user message
        if let Some(MessageContent::Text(text)) = &result.messages[0].content {
            assert!(text.contains("<augmentation>"));
        }
    }

    #[test]
    fn test_inject_augmentation_preserves_fields() {
        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![
                Message {
                    role: "user".to_string(),
                    content: Some(MessageContent::Text("Hello".to_string())),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(100),
            stream: Some(false),
            tools: None,
            tool_choice: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            user: None,
            reasoning_effort: None,
            verbosity: None,
            thinking_budget: None,
        };

        let result = inject_augmentation(request, "Context").unwrap();

        assert_eq!(result.model, "test-model");
        assert_eq!(result.temperature, Some(0.7));
        assert_eq!(result.top_p, Some(0.9));
        assert_eq!(result.max_tokens, Some(100));
    }
}
