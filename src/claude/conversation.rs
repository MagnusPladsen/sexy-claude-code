use crate::claude::events::{ContentBlockType, Delta, StreamEvent};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub enum ContentBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

// ---------------------------------------------------------------------------
// Conversation state
// ---------------------------------------------------------------------------

pub struct Conversation {
    pub messages: Vec<Message>,
    streaming: bool,
    /// Buffer that accumulates partial JSON chunks for tool_use input.
    tool_input_buf: String,
    /// Tracks the ContentBlockType for each block index in the current message,
    /// so we know how to handle deltas and how to finalise blocks on stop.
    block_types: Vec<ContentBlockType>,
}

impl Conversation {
    /// Create an empty conversation.
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            streaming: false,
            tool_input_buf: String::new(),
            block_types: Vec::new(),
        }
    }

    /// Add a user message to the conversation.
    pub fn push_user_message(&mut self, text: String) {
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text(text)],
        });
    }

    /// Process a single stream event, updating the conversation state.
    pub fn apply_event(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::MessageStart { .. } => {
                self.messages.push(Message {
                    role: Role::Assistant,
                    content: Vec::new(),
                });
                self.streaming = true;
                self.block_types.clear();
                self.tool_input_buf.clear();
            }

            StreamEvent::ContentBlockStart { block_type, .. } => {
                if let Some(msg) = self.messages.last_mut() {
                    match block_type {
                        ContentBlockType::Text => {
                            msg.content.push(ContentBlock::Text(String::new()));
                            self.block_types.push(ContentBlockType::Text);
                        }
                        ContentBlockType::ToolUse { id, name } => {
                            msg.content.push(ContentBlock::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                input: String::new(),
                            });
                            self.block_types.push(ContentBlockType::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                            });
                            self.tool_input_buf.clear();
                        }
                    }
                }
            }

            StreamEvent::ContentBlockDelta { index, delta } => {
                if let Some(msg) = self.messages.last_mut() {
                    let idx = *index;
                    match delta {
                        Delta::TextDelta(text) => {
                            if let Some(ContentBlock::Text(ref mut s)) = msg.content.get_mut(idx) {
                                s.push_str(text);
                            }
                        }
                        Delta::InputJsonDelta(partial_json) => {
                            // Accumulate partial JSON in the buffer
                            self.tool_input_buf.push_str(partial_json);
                            // Also update the ToolUse block's input in-place so
                            // callers can inspect partial input while streaming.
                            if let Some(ContentBlock::ToolUse { ref mut input, .. }) =
                                msg.content.get_mut(idx)
                            {
                                *input = self.tool_input_buf.clone();
                            }
                        }
                    }
                }
            }

            StreamEvent::ContentBlockStop { .. } => {
                // Finalisation is already handled incrementally in
                // ContentBlockDelta, so nothing extra is needed here.
            }

            StreamEvent::MessageDelta { .. } => {
                // Could extract stop_reason if needed in the future.
            }

            StreamEvent::MessageStop => {
                self.streaming = false;
            }

            StreamEvent::Unknown(_) => {
                // Ignored.
            }
        }
    }

    /// Whether the conversation is currently receiving a streamed response.
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Returns the text of the last text block in the last assistant message.
    ///
    /// This is useful for rendering the currently-streaming response. Returns
    /// an empty string if there is no assistant message or no text block.
    pub fn streaming_text(&self) -> &str {
        self.messages
            .last()
            .filter(|m| m.role == Role::Assistant)
            .and_then(|m| {
                m.content.iter().rev().find_map(|block| match block {
                    ContentBlock::Text(s) => Some(s.as_str()),
                    _ => None,
                })
            })
            .unwrap_or("")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::events::{ContentBlockType, Delta, StreamEvent};

    #[test]
    fn test_push_user_message() {
        let mut conv = Conversation::new();
        conv.push_user_message("Hello".to_string());

        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, Role::User);
        assert_eq!(conv.messages[0].content.len(), 1);
        match &conv.messages[0].content[0] {
            ContentBlock::Text(t) => assert_eq!(t, "Hello"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_message_start_creates_assistant_message() {
        let mut conv = Conversation::new();
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
        });

        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, Role::Assistant);
        assert!(conv.messages[0].content.is_empty());
        assert!(conv.is_streaming());
    }

    #[test]
    fn test_text_delta_accumulates() {
        let mut conv = Conversation::new();

        // Start a message
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
        });

        // Start a text content block
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Text,
        });

        // Send two text deltas
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("Hello, ".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("world!".to_string()),
        });

        // Verify the text is concatenated
        let msg = &conv.messages[0];
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Text(t) => assert_eq!(t, "Hello, world!"),
            other => panic!("Expected Text, got {:?}", other),
        }

        // streaming_text should return the same
        assert_eq!(conv.streaming_text(), "Hello, world!");
    }

    #[test]
    fn test_message_stop_ends_streaming() {
        let mut conv = Conversation::new();

        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
        });
        assert!(conv.is_streaming());

        conv.apply_event(&StreamEvent::MessageStop);
        assert!(!conv.is_streaming());
    }

    #[test]
    fn test_tool_use_block() {
        let mut conv = Conversation::new();

        // Start message
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
        });

        // Start a tool_use content block
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::ToolUse {
                id: "toolu_abc".to_string(),
                name: "Bash".to_string(),
            },
        });

        // Send partial JSON input deltas
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::InputJsonDelta(r#"{"comm"#.to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::InputJsonDelta(r#"and":"ls"}"#.to_string()),
        });

        // Stop the block
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });

        // Verify the tool use block
        let msg = &conv.messages[0];
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_abc");
                assert_eq!(name, "Bash");
                assert_eq!(input, r#"{"command":"ls"}"#);
            }
            other => panic!("Expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn test_full_conversation_flow() {
        let mut conv = Conversation::new();

        // User sends a message
        conv.push_user_message("What is 2+2?".to_string());

        // Assistant responds
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Text,
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("2+2 = 4".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });
        conv.apply_event(&StreamEvent::MessageDelta {
            stop_reason: Some("end_turn".to_string()),
        });
        conv.apply_event(&StreamEvent::MessageStop);

        // Should have 2 messages total
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, Role::User);
        assert_eq!(conv.messages[1].role, Role::Assistant);
        assert!(!conv.is_streaming());

        // Verify content
        match &conv.messages[1].content[0] {
            ContentBlock::Text(t) => assert_eq!(t, "2+2 = 4"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }
}
