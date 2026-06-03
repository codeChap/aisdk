//! Parse each CLI's JSON/stream output into one provider-agnostic shape.
//!
//! Schemas captured empirically from `claude` 2.1.x and `grok` 0.2.x:
//! - claude json/stream end with `{"type":"result", result, total_cost_usd, usage, modelUsage, ...}`
//! - claude stream text arrives as `stream_event.event.delta.text` (delta.type == "text_delta")
//! - grok json:   `{"text","stopReason","sessionId","thought"}`  (no usage/cost)
//! - grok stream: `{"type":"thought"|"text","data":...}` then `{"type":"end","stopReason","sessionId"}`

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;

/// The unified result of a run. Fields a provider does not report stay `None`.
#[derive(Debug, Default, Clone, Serialize)]
pub struct RunResult {
    pub provider: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

/// One normalized streaming event.
pub enum StreamEvent {
    Init {
        model: Option<String>,
        session_id: Option<String>,
    },
    Reasoning(String),
    Text(String),
    ToolUse(String),
    Done(RunResult),
    Other,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(String::from)
}

fn claude_result(v: &Value) -> RunResult {
    let usage = v.get("usage");
    // Pick the costliest model in modelUsage as the "primary" model.
    let model = v
        .get("modelUsage")
        .and_then(|m| m.as_object())
        .and_then(|o| {
            o.iter()
                .max_by(|a, b| {
                    let ca = a.1.get("costUSD").and_then(|x| x.as_f64()).unwrap_or(0.0);
                    let cb = b.1.get("costUSD").and_then(|x| x.as_f64()).unwrap_or(0.0);
                    ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(k, _)| k.clone())
        });
    RunResult {
        provider: "claude".into(),
        text: v
            .get("result")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string(),
        session_id: s(v, "session_id"),
        stop_reason: s(v, "stop_reason"),
        is_error: v.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false),
        model,
        cost_usd: v.get("total_cost_usd").and_then(|x| x.as_f64()),
        input_tokens: usage
            .and_then(|u| u.get("input_tokens"))
            .and_then(|x| x.as_u64()),
        output_tokens: usage
            .and_then(|u| u.get("output_tokens"))
            .and_then(|x| x.as_u64()),
        ..Default::default()
    }
}

pub fn parse_claude_json(out: &str) -> Result<RunResult> {
    Ok(claude_result(&loose(out)?))
}

pub fn parse_grok_json(out: &str) -> Result<RunResult> {
    let v = loose(out)?;
    Ok(RunResult {
        provider: "grok".into(),
        text: v
            .get("text")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string(),
        reasoning: s(&v, "thought"),
        session_id: s(&v, "sessionId"),
        stop_reason: s(&v, "stopReason"),
        ..Default::default()
    })
}

pub fn parse_claude_event(v: &Value) -> StreamEvent {
    match v.get("type").and_then(|x| x.as_str()) {
        Some("system") if v.get("subtype").and_then(|x| x.as_str()) == Some("init") => {
            StreamEvent::Init {
                model: s(v, "model"),
                session_id: s(v, "session_id"),
            }
        }
        Some("stream_event") => {
            let ev = v.get("event");
            match ev.and_then(|e| e.get("type")).and_then(|x| x.as_str()) {
                Some("content_block_delta") => {
                    let d = ev.and_then(|e| e.get("delta"));
                    match d.and_then(|d| d.get("type")).and_then(|x| x.as_str()) {
                        Some("text_delta") => StreamEvent::Text(
                            d.and_then(|d| d.get("text"))
                                .and_then(|x| x.as_str())
                                .unwrap_or_default()
                                .to_string(),
                        ),
                        Some("thinking_delta") => StreamEvent::Reasoning(
                            d.and_then(|d| d.get("thinking"))
                                .and_then(|x| x.as_str())
                                .unwrap_or_default()
                                .to_string(),
                        ),
                        _ => StreamEvent::Other,
                    }
                }
                Some("content_block_start") => {
                    let cb = ev.and_then(|e| e.get("content_block"));
                    if cb.and_then(|c| c.get("type")).and_then(|x| x.as_str()) == Some("tool_use") {
                        StreamEvent::ToolUse(
                            cb.and_then(|c| c.get("name"))
                                .and_then(|x| x.as_str())
                                .unwrap_or("tool")
                                .to_string(),
                        )
                    } else {
                        StreamEvent::Other
                    }
                }
                _ => StreamEvent::Other,
            }
        }
        Some("result") => StreamEvent::Done(claude_result(v)),
        _ => StreamEvent::Other,
    }
}

pub fn parse_grok_event(v: &Value) -> StreamEvent {
    match v.get("type").and_then(|x| x.as_str()) {
        Some("thought") => StreamEvent::Reasoning(
            v.get("data")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
        ),
        Some("text") => StreamEvent::Text(
            v.get("data")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
        ),
        Some("tool_use") | Some("tool") | Some("tool_call") => StreamEvent::ToolUse(
            v.get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("tool")
                .to_string(),
        ),
        Some("end") => StreamEvent::Done(RunResult {
            provider: "grok".into(),
            session_id: s(v, "sessionId"),
            stop_reason: s(v, "stopReason"),
            ..Default::default()
        }),
        _ => StreamEvent::Other,
    }
}

/// Parse a JSON object that may be padded by stray non-JSON output.
fn loose(out: &str) -> Result<Value> {
    let t = out.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Ok(v);
    }
    let start = t.find('{').ok_or_else(|| anyhow!("no JSON object in output"))?;
    let end = t
        .rfind('}')
        .ok_or_else(|| anyhow!("unterminated JSON object in output"))?;
    if end <= start {
        return Err(anyhow!("malformed JSON in output"));
    }
    Ok(serde_json::from_str::<Value>(&t[start..=end])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false,
        "result":"ok","stop_reason":"end_turn","session_id":"abc123",
        "total_cost_usd":0.05,"usage":{"input_tokens":100,"output_tokens":4},
        "modelUsage":{"claude-haiku-4-5":{"costUSD":0.0005},"claude-opus-4-8":{"costUSD":0.049}}}"#;

    #[test]
    fn claude_json_extracts_cost_tokens_and_costliest_model() {
        let r = parse_claude_json(CLAUDE_RESULT).unwrap();
        assert_eq!(r.provider, "claude");
        assert_eq!(r.text, "ok");
        assert_eq!(r.cost_usd, Some(0.05));
        assert_eq!(r.input_tokens, Some(100));
        assert_eq!(r.output_tokens, Some(4));
        assert_eq!(r.model.as_deref(), Some("claude-opus-4-8")); // costliest, not haiku
        assert_eq!(r.stop_reason.as_deref(), Some("end_turn"));
        assert!(!r.is_error);
    }

    #[test]
    fn grok_json_has_reasoning_and_no_cost() {
        let r =
            parse_grok_json(r#"{"text":"ok","stopReason":"EndTurn","sessionId":"019e","thought":"hmm"}"#)
                .unwrap();
        assert_eq!(r.provider, "grok");
        assert_eq!(r.text, "ok");
        assert_eq!(r.stop_reason.as_deref(), Some("EndTurn"));
        assert_eq!(r.reasoning.as_deref(), Some("hmm"));
        assert_eq!(r.cost_usd, None);
        assert_eq!(r.model, None);
    }

    #[test]
    fn claude_stream_text_delta_and_result() {
        let delta: Value = serde_json::from_str(
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"ok"}}}"#,
        )
        .unwrap();
        assert!(matches!(parse_claude_event(&delta), StreamEvent::Text(t) if t == "ok"));

        let result: Value = serde_json::from_str(CLAUDE_RESULT).unwrap();
        match parse_claude_event(&result) {
            StreamEvent::Done(r) => assert_eq!(r.cost_usd, Some(0.05)),
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn grok_stream_thought_text_end() {
        let thought: Value = serde_json::from_str(r#"{"type":"thought","data":"th"}"#).unwrap();
        let text: Value = serde_json::from_str(r#"{"type":"text","data":"ok"}"#).unwrap();
        let end: Value =
            serde_json::from_str(r#"{"type":"end","stopReason":"EndTurn","sessionId":"019e"}"#).unwrap();
        assert!(matches!(parse_grok_event(&thought), StreamEvent::Reasoning(t) if t == "th"));
        assert!(matches!(parse_grok_event(&text), StreamEvent::Text(t) if t == "ok"));
        match parse_grok_event(&end) {
            StreamEvent::Done(r) => {
                assert_eq!(r.stop_reason.as_deref(), Some("EndTurn"));
                assert_eq!(r.cost_usd, None);
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn loose_tolerates_leading_noise() {
        let r = parse_grok_json("WARN: mcp blah\n{\"text\":\"hi\",\"stopReason\":\"EndTurn\"}").unwrap();
        assert_eq!(r.text, "hi");
    }
}
