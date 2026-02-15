use serde::Deserialize;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum StreamEvent {
    MessageStart {
        message_id: String,
        model: String,
        usage: Option<Usage>,
    },
    ContentBlockStart { index: usize, block_type: ContentBlockType },
    ContentBlockDelta { index: usize, delta: Delta },
    ContentBlockStop { index: usize },
    MessageDelta {
        stop_reason: Option<String>,
        usage: Option<Usage>,
    },
    MessageStop,
    /// System init event carrying slash commands and session metadata.
    SystemInit {
        slash_commands: Vec<String>,
        session_id: Option<String>,
    },
    /// System hook lifecycle event (hook_started, hook_completed).
    SystemHook {
        subtype: String,
        hook_id: Option<String>,
    },
    /// Result event emitted when a command completes (e.g. slash commands).
    Result { text: String, is_error: bool },
    /// Tool result from a `{"type":"user"}` envelope after tool execution.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    Unknown(String),
}

/// Token usage data from message events.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone)]
pub enum ContentBlockType {
    Text,
    ToolUse { id: String, name: String },
    Thinking,
}

#[derive(Debug, Clone)]
pub enum Delta {
    TextDelta(String),
    InputJsonDelta(String),
    ThinkingDelta(String),
}

// ---------------------------------------------------------------------------
// Intermediate serde structs
// ---------------------------------------------------------------------------

/// Top-level envelope from Claude CLI stream-json output.
/// Claude CLI wraps Anthropic streaming events inside `{"type":"stream_event","event":{...}}`.
#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    envelope_type: String,
    /// The inner event when envelope_type == "stream_event"
    event: Option<serde_json::Value>,
    /// Subtype for system events (e.g. "init", "hook_started")
    subtype: Option<String>,
    /// Slash commands from system.init
    slash_commands: Option<Vec<String>>,
    /// Session ID from system.init
    session_id: Option<String>,
    /// Hook ID for system hook events
    hook_id: Option<String>,
    /// Generic message field — used by both "assistant" and "user" envelopes.
    /// Typed as Value because the two formats have different shapes.
    message: Option<serde_json::Value>,
    /// Result text from "result" envelope (slash command output etc.)
    result: Option<String>,
    /// Whether the result is an error
    is_error: Option<bool>,
    /// Structured tool result metadata (e.g. file content, line counts).
    tool_use_result: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawEvent {
    #[serde(rename = "type")]
    event_type: String,
    message: Option<RawMessage>,
    index: Option<usize>,
    content_block: Option<RawContentBlock>,
    delta: Option<RawDelta>,
    usage: Option<RawUsage>,
}

#[derive(Deserialize)]
struct RawMessage {
    id: String,
    model: String,
    usage: Option<RawUsage>,
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct RawContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
    partial_json: Option<String>,
    stop_reason: Option<String>,
    /// Thinking delta text
    thinking: Option<String>,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub fn parse_event(line: &str) -> StreamEvent {
    // First, parse the top-level envelope from Claude CLI.
    // Claude CLI wraps streaming events: {"type":"stream_event","event":{...}}
    // It also emits: {"type":"system",...}, {"type":"assistant",...}, {"type":"result",...}
    let envelope: Envelope = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return StreamEvent::Unknown(line.to_string()),
    };

    match envelope.envelope_type.as_str() {
        "stream_event" => {
            // Unwrap the inner event and parse it
            let inner = match envelope.event {
                Some(v) => v,
                None => return StreamEvent::Unknown(line.to_string()),
            };
            let raw: RawEvent = match serde_json::from_value(inner) {
                Ok(v) => v,
                Err(_) => return StreamEvent::Unknown(line.to_string()),
            };
            parse_raw_event(raw, line)
        }
        // System init carries slash commands and session ID
        "system" if envelope.subtype.as_deref() == Some("init") => {
            StreamEvent::SystemInit {
                slash_commands: envelope.slash_commands.unwrap_or_default(),
                session_id: envelope.session_id,
            }
        }
        // System hook lifecycle events (hook_started, hook_completed)
        "system" => {
            let subtype = envelope.subtype.unwrap_or_default();
            StreamEvent::SystemHook {
                subtype,
                hook_id: envelope.hook_id,
            }
        }
        // Result event carries slash command output
        "result" => {
            let text = envelope.result.unwrap_or_default();
            let is_error = envelope.is_error.unwrap_or(false);
            StreamEvent::Result { text, is_error }
        }
        // Tool result from tool execution — emitted as {"type":"user","message":{...}}
        "user" => parse_tool_result(&envelope, line),
        // Full assistant message — we use streaming events instead
        "assistant" => StreamEvent::Unknown(line.to_string()),
        // Try parsing as a raw event directly (for backwards compatibility / tests)
        _ => {
            let raw: RawEvent = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => return StreamEvent::Unknown(line.to_string()),
            };
            parse_raw_event(raw, line)
        }
    }
}

/// Parse a tool result from a `{"type":"user"}` envelope.
///
/// The envelope carries tool execution results:
/// ```json
/// {"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"...","content":"..."}]},
///  "tool_use_result":{"type":"text","file":{"filePath":"...","content":"..."}}}
/// ```
///
/// Prefers clean content from `tool_use_result` metadata when available.
fn parse_tool_result(envelope: &Envelope, line: &str) -> StreamEvent {
    let msg = match envelope.message.as_ref() {
        Some(v) => v,
        None => return StreamEvent::Unknown(line.to_string()),
    };

    let content_arr = match msg.get("content").and_then(|c| c.as_array()) {
        Some(arr) => arr,
        None => return StreamEvent::Unknown(line.to_string()),
    };

    // Find the first tool_result in the content array
    for item in content_arr {
        if item.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
            continue;
        }

        let tool_use_id = item
            .get("tool_use_id")
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string();

        let is_error = item
            .get("is_error")
            .and_then(|e| e.as_bool())
            .unwrap_or(false);

        // Prefer clean content from tool_use_result metadata
        let content = extract_clean_content(envelope)
            .or_else(|| {
                item.get("content")
                    .and_then(|c| c.as_str())
                    .map(String::from)
            })
            .unwrap_or_default();

        return StreamEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
        };
    }

    StreamEvent::Unknown(line.to_string())
}

/// Extract clean content from the `tool_use_result` metadata field.
/// This avoids line-number prefixes present in the raw content.
fn extract_clean_content(envelope: &Envelope) -> Option<String> {
    let meta = envelope.tool_use_result.as_ref()?;
    // File tool results: {"type":"text","file":{"content":"..."}}
    if let Some(content) = meta.get("file").and_then(|f| f.get("content")).and_then(|c| c.as_str())
    {
        return Some(content.to_string());
    }
    // Direct text results: {"type":"text","text":"..."}
    if let Some(content) = meta.get("text").and_then(|t| t.as_str()) {
        return Some(content.to_string());
    }
    None
}

/// Parse the inner Anthropic streaming event.
fn parse_raw_event(raw: RawEvent, line: &str) -> StreamEvent {
    match raw.event_type.as_str() {
        "message_start" => {
            if let Some(msg) = raw.message {
                let usage = msg.usage.map(|u| Usage {
                    input_tokens: u.input_tokens.unwrap_or(0),
                    output_tokens: u.output_tokens.unwrap_or(0),
                });
                StreamEvent::MessageStart {
                    message_id: msg.id,
                    model: msg.model,
                    usage,
                }
            } else {
                StreamEvent::Unknown(line.to_string())
            }
        }

        "content_block_start" => {
            let index = raw.index.unwrap_or(0);
            if let Some(block) = raw.content_block {
                let block_type = match block.block_type.as_str() {
                    "text" => ContentBlockType::Text,
                    "tool_use" => ContentBlockType::ToolUse {
                        id: block.id.unwrap_or_default(),
                        name: block.name.unwrap_or_default(),
                    },
                    "thinking" => ContentBlockType::Thinking,
                    _ => return StreamEvent::Unknown(line.to_string()),
                };
                StreamEvent::ContentBlockStart { index, block_type }
            } else {
                StreamEvent::Unknown(line.to_string())
            }
        }

        "content_block_delta" => {
            let index = raw.index.unwrap_or(0);
            if let Some(d) = raw.delta {
                let delta = match d.delta_type.as_deref() {
                    Some("text_delta") => Delta::TextDelta(d.text.unwrap_or_default()),
                    Some("input_json_delta") => {
                        Delta::InputJsonDelta(d.partial_json.unwrap_or_default())
                    }
                    Some("thinking_delta") => {
                        Delta::ThinkingDelta(d.thinking.or(d.text).unwrap_or_default())
                    }
                    _ => return StreamEvent::Unknown(line.to_string()),
                };
                StreamEvent::ContentBlockDelta { index, delta }
            } else {
                StreamEvent::Unknown(line.to_string())
            }
        }

        "content_block_stop" => {
            let index = raw.index.unwrap_or(0);
            StreamEvent::ContentBlockStop { index }
        }

        "message_delta" => {
            let usage = raw.usage.map(|u| Usage {
                input_tokens: u.input_tokens.unwrap_or(0),
                output_tokens: u.output_tokens.unwrap_or(0),
            });
            let stop_reason = raw.delta.and_then(|d| d.stop_reason);
            StreamEvent::MessageDelta { stop_reason, usage }
        }

        "message_stop" => StreamEvent::MessageStop,

        _ => StreamEvent::Unknown(line.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Claude CLI envelope format (stream_event wrapper) ---

    #[test]
    fn test_parse_stream_event_message_start() {
        let line = r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","content":[],"model":"claude-opus-4-6","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}},"session_id":"abc","uuid":"def"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageStart { message_id, model, .. } => {
                assert_eq!(message_id, "msg_123");
                assert_eq!(model, "claude-opus-4-6");
            }
            other => panic!("Expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_stream_event_content_block_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}},"session_id":"abc","uuid":"def"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    Delta::TextDelta(text) => assert_eq!(text, "Hello"),
                    other => panic!("Expected TextDelta, got {:?}", other),
                }
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_stream_event_message_stop() {
        let line = r#"{"type":"stream_event","event":{"type":"message_stop"},"session_id":"abc","uuid":"def"}"#;
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::MessageStop));
    }

    #[test]
    fn test_parse_stream_event_content_block_start_text() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}},"session_id":"abc","uuid":"def"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockStart { index, block_type } => {
                assert_eq!(index, 0);
                assert!(matches!(block_type, ContentBlockType::Text));
            }
            other => panic!("Expected ContentBlockStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_stream_event_content_block_stop() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"abc","uuid":"def"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockStop { index } => assert_eq!(index, 0),
            other => panic!("Expected ContentBlockStop, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_stream_event_message_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":42}},"session_id":"abc","uuid":"def"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageDelta { stop_reason, .. } => {
                assert_eq!(stop_reason, Some("end_turn".to_string()));
            }
            other => panic!("Expected MessageDelta, got {:?}", other),
        }
    }

    // --- System/result/assistant events are treated as Unknown ---

    #[test]
    fn test_parse_system_init_extracts_slash_commands() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/tmp","session_id":"abc-123","slash_commands":["commit","review","brainstorm"]}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::SystemInit {
                slash_commands,
                session_id,
            } => {
                assert_eq!(slash_commands, vec!["commit", "review", "brainstorm"]);
                assert_eq!(session_id, Some("abc-123".to_string()));
            }
            other => panic!("Expected SystemInit, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_system_hook_event() {
        let line = r#"{"type":"system","subtype":"hook_started","hook_id":"abc","session_id":"def"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::SystemHook { subtype, hook_id } => {
                assert_eq!(subtype, "hook_started");
                assert_eq!(hook_id, Some("abc".to_string()));
            }
            other => panic!("Expected SystemHook, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_result_event() {
        let line = r#"{"type":"result","subtype":"success","result":"Hello","session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::Result { text, is_error } => {
                assert_eq!(text, "Hello");
                assert!(!is_error);
            }
            other => panic!("Expected Result, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_result_event_error() {
        let line = r#"{"type":"result","subtype":"error","result":"Something failed","is_error":true,"session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::Result { text, is_error } => {
                assert_eq!(text, "Something failed");
                assert!(is_error);
            }
            other => panic!("Expected Result, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_assistant_event_is_unknown() {
        let line = r#"{"type":"assistant","message":{"id":"msg_1","model":"claude-opus-4-6","type":"message","role":"assistant","content":[{"type":"text","text":"Hi"}]},"session_id":"abc"}"#;
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::Unknown(_)));
    }

    // --- Backwards-compat: raw Anthropic format still works ---

    #[test]
    fn test_parse_raw_message_start() {
        let line = r#"{"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","content":[],"model":"claude-opus-4-6","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageStart { message_id, model, .. } => {
                assert_eq!(message_id, "msg_123");
                assert_eq!(model, "claude-opus-4-6");
            }
            other => panic!("Expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_raw_content_block_delta_text() {
        let line = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello world"}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    Delta::TextDelta(text) => assert_eq!(text, "Hello world"),
                    other => panic!("Expected TextDelta, got {:?}", other),
                }
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    // --- Tool result events ---

    #[test]
    fn test_parse_tool_result_with_file_metadata() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_abc","type":"tool_result","content":"     1\u2192hello world\n     2\u2192"}]},"tool_use_result":{"type":"text","file":{"filePath":"/tmp/test.txt","content":"hello world\n","numLines":2,"startLine":1,"totalLines":2}},"session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_abc");
                // Should use clean content from metadata
                assert_eq!(content, "hello world\n");
                assert!(!is_error);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tool_result_without_metadata() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_def","type":"tool_result","content":"command output here"}]},"session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_def");
                assert_eq!(content, "command output here");
                assert!(!is_error);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tool_result_error() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_err","type":"tool_result","content":"Error: file not found","is_error":true}]},"session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_err");
                assert_eq!(content, "Error: file not found");
                assert!(is_error);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_user_event_without_tool_result_is_unknown() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]},"session_id":"abc"}"#;
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::Unknown(_)));
    }

    // --- Thinking blocks ---

    #[test]
    fn test_parse_thinking_content_block_start() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}},"session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockStart { index, block_type } => {
                assert_eq!(index, 0);
                assert!(matches!(block_type, ContentBlockType::Thinking));
            }
            other => panic!("Expected ContentBlockStart(Thinking), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_thinking_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think..."}},"session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    Delta::ThinkingDelta(text) => assert_eq!(text, "Let me think..."),
                    other => panic!("Expected ThinkingDelta, got {:?}", other),
                }
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    // --- Usage extraction ---

    #[test]
    fn test_message_start_extracts_usage() {
        let line = r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","content":[],"model":"claude-opus-4-6","stop_reason":null,"usage":{"input_tokens":100,"output_tokens":5}}},"session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageStart { usage, .. } => {
                let u = usage.expect("Expected usage data");
                assert_eq!(u.input_tokens, 100);
                assert_eq!(u.output_tokens, 5);
            }
            other => panic!("Expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn test_message_delta_extracts_usage() {
        let line = r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":42}},"session_id":"abc"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageDelta { usage, .. } => {
                let u = usage.expect("Expected usage data");
                assert_eq!(u.output_tokens, 42);
            }
            other => panic!("Expected MessageDelta, got {:?}", other),
        }
    }

    // --- Edge cases ---

    #[test]
    fn test_parse_invalid_json() {
        let line = "this is not json at all";
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::Unknown(_)));
    }

    #[test]
    fn test_parse_unknown_event_type() {
        let line = r#"{"type":"ping"}"#;
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::Unknown(_)));
    }
}
