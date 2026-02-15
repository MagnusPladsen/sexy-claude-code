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
#[allow(dead_code)]
pub enum ContentBlock {
    Text(String),
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        /// Whether this result is collapsed in the UI (auto-collapsed if >20 lines).
        collapsed: bool,
    },
    /// Image content block (rendered as placeholder in terminal).
    Image {
        media_type: String,
    },
    /// Document content block (rendered as placeholder in terminal).
    Document {
        doc_type: String,
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
    /// Set to true when a full streaming response completes (MessageStop).
    /// Used to suppress duplicate messages from the Result event that follows.
    had_streaming_response: bool,
    /// True when tool execution is in progress (between MessageStop with
    /// a ToolUse block and the arrival of a ToolResult or new MessageStart).
    awaiting_tool_result: bool,
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
            had_streaming_response: false,
            awaiting_tool_result: false,
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

    /// Add a system/info message displayed as an assistant message.
    pub fn push_system_message(&mut self, text: String) {
        self.messages.push(Message {
            role: Role::Assistant,
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
                self.had_streaming_response = false;
                self.awaiting_tool_result = false;
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
                        ContentBlockType::Thinking => {
                            msg.content.push(ContentBlock::Thinking(String::new()));
                            self.block_types.push(ContentBlockType::Thinking);
                        }
                        ContentBlockType::Image { ref media_type } => {
                            msg.content.push(ContentBlock::Image {
                                media_type: media_type.clone(),
                            });
                            self.block_types.push(block_type.clone());
                        }
                        ContentBlockType::Document { ref doc_type } => {
                            msg.content.push(ContentBlock::Document {
                                doc_type: doc_type.clone(),
                            });
                            self.block_types.push(block_type.clone());
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
                        Delta::ThinkingDelta(text) => {
                            if let Some(ContentBlock::Thinking(ref mut s)) =
                                msg.content.get_mut(idx)
                            {
                                s.push_str(text);
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
                self.had_streaming_response = true;
                // Check if the last content block is a ToolUse — if so,
                // tool execution is about to happen.
                let has_pending_tool = self
                    .messages
                    .last()
                    .and_then(|m| m.content.last())
                    .is_some_and(|b| matches!(b, ContentBlock::ToolUse { .. }));
                if has_pending_tool {
                    self.awaiting_tool_result = true;
                }
            }

            StreamEvent::Result { ref text, .. } => {
                // For normal responses, streaming events already built the message,
                // so the Result is a duplicate — skip it.
                // For slash commands (no streaming), Result is the only source.
                if !text.is_empty() && !self.had_streaming_response {
                    self.messages.push(Message {
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text(text.clone())],
                    });
                }
                self.streaming = false;
                self.had_streaming_response = false;
            }

            StreamEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                self.awaiting_tool_result = false;
                // Append tool result to the last assistant message.
                // The renderer matches it to its ToolUse by ID.
                if let Some(msg) = self.messages.last_mut() {
                    let collapsed = content.lines().count() > 20;
                    msg.content.push(ContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                        collapsed,
                    });
                }
            }

            StreamEvent::SystemInit { .. }
            | StreamEvent::SystemHook { .. }
            | StreamEvent::Unknown(_) => {
                // Handled by App, not conversation state.
            }
        }
    }

    /// Whether the conversation is currently receiving a streamed response.
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Whether a tool is currently executing (between MessageStop and ToolResult).
    pub fn is_awaiting_tool_result(&self) -> bool {
        self.awaiting_tool_result
    }

    /// Returns the text of the last text block in the last assistant message.
    ///
    /// This is useful for rendering the currently-streaming response. Returns
    /// an empty string if there is no assistant message or no text block.
    #[allow(dead_code)]
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
            usage: None,
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
            usage: None,
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
            usage: None,
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
            usage: None,
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
            usage: None,
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
            usage: None,
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

    #[test]
    fn test_result_after_streaming_does_not_duplicate() {
        let mut conv = Conversation::new();

        // User sends a message
        conv.push_user_message("Hello".to_string());

        // Full streaming response
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: None,
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Text,
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("Hi there!".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });
        conv.apply_event(&StreamEvent::MessageStop);

        // Result event with the same text (Claude CLI always sends this)
        conv.apply_event(&StreamEvent::Result {
            text: "Hi there!".to_string(),
            is_error: false,
        });

        // Should have exactly 2 messages: user + assistant (NOT 3)
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, Role::User);
        assert_eq!(conv.messages[1].role, Role::Assistant);
    }

    #[test]
    fn test_tool_result_appended_to_message() {
        let mut conv = Conversation::new();

        // Start message with a tool use
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: None,
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::ToolUse {
                id: "toolu_abc".to_string(),
                name: "Read".to_string(),
            },
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::InputJsonDelta(r#"{"file_path":"test.txt"}"#.to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });

        // Tool result arrives
        conv.apply_event(&StreamEvent::ToolResult {
            tool_use_id: "toolu_abc".to_string(),
            content: "hello world\n".to_string(),
            is_error: false,
        });

        let msg = &conv.messages[0];
        assert_eq!(msg.content.len(), 2);
        match &msg.content[1] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
                collapsed,
            } => {
                assert_eq!(tool_use_id, "toolu_abc");
                assert_eq!(content, "hello world\n");
                assert!(!is_error);
                assert!(!collapsed); // <20 lines, not collapsed
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_result_long_output_auto_collapsed() {
        let mut conv = Conversation::new();

        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: None,
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::ToolUse {
                id: "toolu_long".to_string(),
                name: "Bash".to_string(),
            },
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });

        // 30-line output should auto-collapse
        let long_output = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        conv.apply_event(&StreamEvent::ToolResult {
            tool_use_id: "toolu_long".to_string(),
            content: long_output,
            is_error: false,
        });

        match &conv.messages[0].content[1] {
            ContentBlock::ToolResult { collapsed, .. } => assert!(collapsed),
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_slash_command_result_creates_message() {
        let mut conv = Conversation::new();

        // Slash command result (no preceding streaming events)
        conv.apply_event(&StreamEvent::Result {
            text: "Available commands: /help, /clear".to_string(),
            is_error: false,
        });

        // Should create one assistant message
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, Role::Assistant);
        match &conv.messages[0].content[0] {
            ContentBlock::Text(t) => assert_eq!(t, "Available commands: /help, /clear"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_thinking_block_accumulated() {
        let mut conv = Conversation::new();
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: None,
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Thinking,
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::ThinkingDelta("Let me ".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::ThinkingDelta("think about this.".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });

        let msg = &conv.messages[0];
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Thinking(t) => assert_eq!(t, "Let me think about this."),
            other => panic!("Expected Thinking, got {:?}", other),
        }
    }

    #[test]
    fn test_awaiting_tool_result_lifecycle() {
        let mut conv = Conversation::new();
        assert!(!conv.is_awaiting_tool_result());

        // Start message with a tool use
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: None,
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::ToolUse {
                id: "toolu_abc".to_string(),
                name: "Bash".to_string(),
            },
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });

        // MessageStop with a ToolUse as last block → awaiting
        conv.apply_event(&StreamEvent::MessageStop);
        assert!(conv.is_awaiting_tool_result());

        // ToolResult clears awaiting state
        conv.apply_event(&StreamEvent::ToolResult {
            tool_use_id: "toolu_abc".to_string(),
            content: "output".to_string(),
            is_error: false,
        });
        assert!(!conv.is_awaiting_tool_result());
    }

    #[test]
    fn test_message_stop_without_tool_use_not_awaiting() {
        let mut conv = Conversation::new();

        // Start message with text only
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: None,
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Text,
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("Hello".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });
        conv.apply_event(&StreamEvent::MessageStop);

        // Text-only message → not awaiting tool result
        assert!(!conv.is_awaiting_tool_result());
    }

    #[test]
    fn test_image_block_added_to_message() {
        let mut conv = Conversation::new();
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: None,
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Image {
                media_type: "image/png".to_string(),
            },
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });

        let msg = &conv.messages[0];
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Image { media_type } => assert_eq!(media_type, "image/png"),
            other => panic!("Expected Image, got {:?}", other),
        }
    }

    #[test]
    fn test_document_block_added_to_message() {
        let mut conv = Conversation::new();
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_001".to_string(),
            model: "claude-opus-4-6".to_string(),
            usage: None,
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Document {
                doc_type: "application/pdf".to_string(),
            },
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });

        let msg = &conv.messages[0];
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Document { doc_type } => assert_eq!(doc_type, "application/pdf"),
            other => panic!("Expected Document, got {:?}", other),
        }
    }
}
