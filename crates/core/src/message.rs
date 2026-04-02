use serde::{Deserialize, Serialize};

use clawcr_provider::{RequestContent, RequestMessage};

/// Conversation role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// A content block within a conversation message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// A single message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// Extract all tool_use blocks from this message.
    pub fn tool_uses(&self) -> Vec<(&str, &str, &serde_json::Value)> {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.as_str(), name.as_str(), input))
                }
                _ => None,
            })
            .collect()
    }

    /// Convert to the provider request format.
    pub fn to_request_message(&self) -> RequestMessage {
        let content = self
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => RequestContent::Text { text: text.clone() },
                ContentBlock::ToolUse { id, name, input } => RequestContent::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                },
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => RequestContent::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error: if *is_error { Some(true) } else { None },
                },
            })
            .collect();

        RequestMessage {
            role: self.role.as_str().to_string(),
            content,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn role_as_str() {
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Assistant.as_str(), "assistant");
    }

    #[test]
    fn role_serde_roundtrip() {
        let json = serde_json::to_string(&Role::User).unwrap();
        assert_eq!(json, r#""user""#);
        let deserialized: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Role::User);
    }

    #[test]
    fn message_user_creates_text_block() {
        let msg = Message::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn message_assistant_text() {
        let msg = Message::assistant_text("response");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "response"),
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn tool_uses_extracts_only_tool_use_blocks() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "thinking...".into(),
                },
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "bash".into(),
                    input: json!({"cmd": "ls"}),
                },
                ContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "grep".into(),
                    input: json!({"pattern": "foo"}),
                },
            ],
        };
        let uses = msg.tool_uses();
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].0, "t1");
        assert_eq!(uses[0].1, "bash");
        assert_eq!(uses[1].0, "t2");
        assert_eq!(uses[1].1, "grep");
    }

    #[test]
    fn tool_uses_empty_when_no_tool_blocks() {
        let msg = Message::user("no tools here");
        assert!(msg.tool_uses().is_empty());
    }

    #[test]
    fn to_request_message_maps_role_and_content() {
        let msg = Message {
            role: Role::User,
            content: vec![
                ContentBlock::Text {
                    text: "hello".into(),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "result".into(),
                    is_error: false,
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t2".into(),
                    content: "error".into(),
                    is_error: true,
                },
            ],
        };
        let req = msg.to_request_message();
        assert_eq!(req.role, "user");
        assert_eq!(req.content.len(), 3);

        match &req.content[1] {
            RequestContent::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, None);
            }
            _ => panic!("expected ToolResult"),
        }
        match &req.content[2] {
            RequestContent::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, Some(true));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn content_block_serde_roundtrip() {
        let block = ContentBlock::ToolUse {
            id: "id1".into(),
            name: "bash".into(),
            input: json!({"cmd": "echo hi"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        match deserialized {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "id1");
                assert_eq!(name, "bash");
                assert_eq!(input, json!({"cmd": "echo hi"}));
            }
            _ => panic!("expected ToolUse"),
        }
    }
}
