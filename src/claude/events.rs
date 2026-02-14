use serde::Deserialize;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum StreamEvent {
    MessageStart { message_id: String, model: String },
    ContentBlockStart { index: usize, block_type: ContentBlockType },
    ContentBlockDelta { index: usize, delta: Delta },
    ContentBlockStop { index: usize },
    MessageDelta { stop_reason: Option<String> },
    MessageStop,
    Unknown(String),
}

#[derive(Debug, Clone)]
pub enum ContentBlockType {
    Text,
    ToolUse { id: String, name: String },
}

#[derive(Debug, Clone)]
pub enum Delta {
    TextDelta(String),
    InputJsonDelta(String),
}

// ---------------------------------------------------------------------------
// Intermediate serde structs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawEvent {
    #[serde(rename = "type")]
    event_type: String,
    message: Option<RawMessage>,
    index: Option<usize>,
    content_block: Option<RawContentBlock>,
    delta: Option<RawDelta>,
    #[allow(dead_code)]
    usage: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawMessage {
    id: String,
    model: String,
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
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub fn parse_event(line: &str) -> StreamEvent {
    let raw: RawEvent = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return StreamEvent::Unknown(line.to_string()),
    };

    match raw.event_type.as_str() {
        "message_start" => {
            if let Some(msg) = raw.message {
                StreamEvent::MessageStart {
                    message_id: msg.id,
                    model: msg.model,
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
            let stop_reason = raw.delta.and_then(|d| d.stop_reason);
            StreamEvent::MessageDelta { stop_reason }
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

    #[test]
    fn test_parse_message_start() {
        let line = r#"{"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","content":[],"model":"claude-opus-4-6","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageStart { message_id, model } => {
                assert_eq!(message_id, "msg_123");
                assert_eq!(model, "claude-opus-4-6");
            }
            other => panic!("Expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_content_block_start_text() {
        let line = r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
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
    fn test_parse_content_block_start_tool_use() {
        let line = r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"Bash","input":{}}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockStart { index, block_type } => {
                assert_eq!(index, 1);
                match block_type {
                    ContentBlockType::ToolUse { id, name } => {
                        assert_eq!(id, "toolu_123");
                        assert_eq!(name, "Bash");
                    }
                    other => panic!("Expected ToolUse, got {:?}", other),
                }
            }
            other => panic!("Expected ContentBlockStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_content_block_delta_text() {
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

    #[test]
    fn test_parse_content_block_delta_input_json() {
        let line = r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"ls\"}"}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 1);
                match delta {
                    Delta::InputJsonDelta(json) => {
                        assert_eq!(json, r#"{"command":"ls"}"#);
                    }
                    other => panic!("Expected InputJsonDelta, got {:?}", other),
                }
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_content_block_stop() {
        let line = r#"{"type":"content_block_stop","index":0}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockStop { index } => {
                assert_eq!(index, 0);
            }
            other => panic!("Expected ContentBlockStop, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_message_delta() {
        let line = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":42}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageDelta { stop_reason } => {
                assert_eq!(stop_reason, Some("end_turn".to_string()));
            }
            other => panic!("Expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_message_stop() {
        let line = r#"{"type":"message_stop"}"#;
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::MessageStop));
    }

    #[test]
    fn test_parse_unknown_event() {
        let line = r#"{"type":"ping"}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::Unknown(raw) => {
                assert_eq!(raw, line);
            }
            other => panic!("Expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_invalid_json() {
        let line = "this is not json at all";
        let event = parse_event(line);
        match event {
            StreamEvent::Unknown(raw) => {
                assert_eq!(raw, line);
            }
            other => panic!("Expected Unknown, got {:?}", other),
        }
    }
}
