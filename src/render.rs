//! Presentation: turn the runtime plan and unified events/results into output.
//! The pure formatters (`summary`, `plan`) are unit-tested; `StreamSink` drives
//! the live streaming view (answer → stdout, decorations → stderr).

use std::io::Write;

use crate::event::{RunResult, StreamEvent};
use crate::provider::{Billing, Request};
use crate::style::Style;

/// First 8 chars of an id, for compact display.
fn short(s: &str) -> String {
    s.chars().take(8).collect()
}

/// The `── provider · model · $cost · in→out tok · stop · session` line.
pub fn summary(r: &RunResult) -> String {
    let mut parts = vec![format!("── {}", r.provider)];
    if let Some(m) = &r.model {
        parts.push(m.clone());
    }
    if let Some(c) = r.cost_usd {
        parts.push(format!("${c:.4}"));
    }
    if let (Some(i), Some(o)) = (r.input_tokens, r.output_tokens) {
        parts.push(format!("{i}→{o} tok"));
    }
    if let Some(sr) = &r.stop_reason {
        parts.push(sr.clone());
    }
    if let Some(s) = &r.session_id {
        parts.push(short(s));
    }
    parts.join(" · ")
}

/// The `--dry-run` / `--verbose` command echo (lines for stderr).
pub fn plan(st: Style, r: &Request, bin: &str, args: &[String]) -> Vec<String> {
    let mut lines = vec![st.dim(&format!("→ {} {}", bin, join(args)))];
    lines.push(st.dim(&match r.billing {
        Billing::Subscription => format!(
            "  billing=subscription → unset for child: {}",
            r.provider.api_key_vars().join(", ")
        ),
        Billing::Api => "  billing=api → API-key env vars passed through".to_string(),
    }));
    lines
}

fn join(args: &[String]) -> String {
    args.iter()
        .map(|x| if x.contains(' ') { format!("{x:?}") } else { x.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renders a streaming run incrementally: answer text to stdout as it arrives,
/// init/reasoning/tool decorations and the final summary to stderr.
pub struct StreamSink {
    st: Style,
    provider: String,
    verbose: bool,
    answer: String,
    result: Option<RunResult>,
}

impl StreamSink {
    pub fn new(provider: &str, verbose: bool) -> Self {
        Self {
            st: Style::for_stderr(),
            provider: provider.to_string(),
            verbose,
            answer: String::new(),
            result: None,
        }
    }

    pub fn handle(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::Init { model, session_id } => {
                if self.st.color() {
                    let m = model.unwrap_or_default();
                    let sid = session_id
                        .map(|s| format!("  {}", short(&s)))
                        .unwrap_or_default();
                    eprintln!("{}", self.st.dim(&format!("◆ {} {}{}", self.provider, m, sid)));
                }
            }
            StreamEvent::Reasoning(t) => {
                if self.verbose && self.st.color() {
                    eprint!("{}", self.st.dim(&t));
                    let _ = std::io::stderr().flush();
                }
            }
            StreamEvent::Text(t) => {
                self.answer.push_str(&t);
                print!("{t}");
                let _ = std::io::stdout().flush();
            }
            StreamEvent::ToolUse(name) => {
                if self.st.color() {
                    eprintln!("{}", self.st.dim(&format!("⚙ {name}")));
                }
            }
            StreamEvent::Done(mut res) => {
                if res.text.is_empty() {
                    res.text = self.answer.clone();
                }
                self.result = Some(res);
            }
            StreamEvent::Other => {}
        }
    }

    /// Emit the trailing newline and summary line. Call after the child exits.
    pub fn finish(self) {
        if !self.answer.ends_with('\n') {
            println!();
        }
        if let Some(res) = &self.result {
            eprintln!("{}", self.st.dim(&summary(res)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_claude_has_cost_tokens_and_truncated_session() {
        let r = RunResult {
            provider: "claude".into(),
            text: "ok".into(),
            model: Some("claude-opus-4-8".into()),
            cost_usd: Some(0.0444),
            input_tokens: Some(6792),
            output_tokens: Some(4),
            stop_reason: Some("end_turn".into()),
            session_id: Some("da23e379aaaa".into()),
            ..Default::default()
        };
        let s = summary(&r);
        assert!(s.contains("── claude"));
        assert!(s.contains("$0.0444"));
        assert!(s.contains("6792→4 tok"));
        assert!(s.contains("da23e379")); // session truncated to 8
        assert!(!s.contains("da23e379a")); // ...not 9
    }

    #[test]
    fn summary_grok_omits_cost_and_tokens() {
        let r = RunResult {
            provider: "grok".into(),
            text: "ok".into(),
            stop_reason: Some("EndTurn".into()),
            session_id: Some("019e8c6c11".into()),
            ..Default::default()
        };
        let s = summary(&r);
        assert!(s.contains("── grok"));
        assert!(!s.contains('$'));
        assert!(!s.contains("tok"));
    }
}
