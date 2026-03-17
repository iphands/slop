//! Extract user content from messages

use crate::api::Message;

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
