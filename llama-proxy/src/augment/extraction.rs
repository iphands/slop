//! Extract user content from messages

use crate::api::Message;

/// Extract user content directly from raw JSON request value.
///
/// Handles both OpenAI format (messages[].content as string or array of parts)
/// and Anthropic format (messages[].content as string or array of content blocks).
/// Returns the last user message text, or all user messages joined by newline.
///
/// This avoids typed deserialization failures caused by unknown fields, strict
/// enum variants, or content that doesn't match expected shapes.
pub fn extract_user_content_raw(json: &serde_json::Value, is_anthropic: bool) -> String {
    let messages = match json.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return String::new(),
    };

    let texts: Vec<String> = messages
        .iter()
        .filter(|msg| msg.get("role").and_then(|r| r.as_str()) == Some("user"))
        .filter_map(|msg| {
            let content = msg.get("content")?;
            let text = extract_text_from_content_value(content, is_anthropic);
            if text.is_empty() { None } else { Some(text) }
        })
        .collect();

    texts.join("\n")
}

/// Extract text from a content JSON value (string or array of blocks/parts).
fn extract_text_from_content_value(content: &serde_json::Value, is_anthropic: bool) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => {
            parts
                .iter()
                .filter_map(|part| {
                    let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if is_anthropic {
                        // Anthropic: text blocks have type "text" and field "text"
                        if part_type == "text" {
                            part.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    } else {
                        // OpenAI: content parts have type "text" and field "text"
                        if part_type == "text" {
                            part.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => String::new(),
    }
}

/// Extract user message content from a list of messages
///
/// Returns all user message contents as a Vec of strings.
/// This is used to send to augment-backend for enrichment.
pub fn extract_user_content(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .filter(|msg| msg.role == "user")
        .filter_map(|msg| match &msg.content {
            Some(content) => Some(content_to_string(content)),
            None => None,
        })
        .collect()
}

/// Convert MessageContent to string
fn content_to_string(content: &crate::api::MessageContent) -> String {
    match content {
        crate::api::MessageContent::Text(text) => text.clone(),
        crate::api::MessageContent::Parts(parts) => {
            parts
                .iter()
                .filter_map(|part| part.text.clone())
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_user_content_simple() {
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(crate::api::MessageContent::Text("You are helpful".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(crate::api::MessageContent::Text("Hello".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some(crate::api::MessageContent::Text("Hi there!".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let result = extract_user_content(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Hello");
    }

    #[test]
    fn test_extract_user_content_multiple() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: Some(crate::api::MessageContent::Text("First message".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some(crate::api::MessageContent::Text("Response".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(crate::api::MessageContent::Text("Second message".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let result = extract_user_content(&messages);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "First message");
        assert_eq!(result[1], "Second message");
    }

    #[test]
    fn test_extract_user_content_empty() {
        let messages: Vec<Message> = vec![];
        let result = extract_user_content(&messages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_user_content_no_user() {
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(crate::api::MessageContent::Text("System".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some(crate::api::MessageContent::Text("Hello".to_string())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let result = extract_user_content(&messages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_user_content_multimodal() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: Some(crate::api::MessageContent::Parts(vec![
                    crate::api::ContentPart {
                        content_type: "text".to_string(),
                        text: Some("What is this?".to_string()),
                        image_url: None,
                    },
                    crate::api::ContentPart {
                        content_type: "image".to_string(),
                        text: None,
                        image_url: Some(crate::api::ImageUrl {
                            url: "https://example.com/image.jpg".to_string(),
                            detail: None,
                        }),
                    },
                ])),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let result = extract_user_content(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "What is this?");
    }
}
