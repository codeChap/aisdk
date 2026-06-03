//! Providers are described by data, not code. Each `Provider` maps to a static
//! `Dialect` (flag names + a couple of structural knobs + parser fn-pointers).
//! A single `build_argv` consumes any dialect, so adding a provider is one
//! `Dialect` literal — no new match arms in argv, exec, or doctor.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Result};
use clap::ValueEnum;
use serde_json::Value;

use crate::event::{self, RunResult, StreamEvent};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Provider {
    #[default]
    Claude,
    Grok,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, ValueEnum)]
pub enum Billing {
    /// Use the CLI's OAuth login; strip API-key env vars from the child.
    #[default]
    Subscription,
    /// Pass API-key env vars through; the CLI bills the metered API.
    Api,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, ValueEnum)]
pub enum Format {
    #[default]
    Text,
    Json,
    Stream,
}

/// A provider-agnostic request, mapped to each CLI's native flags by `argv`.
#[derive(Debug, Default)]
pub struct Request {
    pub provider: Provider,
    pub model: Option<String>,
    pub prompt: String,
    pub system: Option<String>,
    pub append_system: Option<String>,
    pub cwd: Option<PathBuf>,
    pub billing: Billing,
    pub format: Format,
    pub yolo: bool,
    pub permission_mode: Option<String>,
    pub continue_session: bool,
    pub resume: Option<String>,
    pub max_turns: Option<u32>,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub passthrough: Vec<String>,
    pub verbose: bool,
    pub dry_run: bool,
}

/// How a CLI takes tool allow/deny lists.
pub enum ToolStyle {
    /// One flag, comma-joined values: `--allowedTools a,b`
    Joined {
        allow: &'static str,
        deny: &'static str,
    },
    /// Flag repeated per value: `--allow a --allow b`
    Repeated {
        allow: &'static str,
        deny: &'static str,
    },
}

/// How a CLI takes the prompt.
pub enum PromptStyle {
    /// Trailing positional argument.
    Positional,
    /// Behind a flag, e.g. `--single <prompt>`.
    Flag(&'static str),
}

/// Everything provider-specific, as data.
pub struct Dialect {
    pub print_flag: Option<&'static str>,
    pub model: &'static str,
    pub system: &'static str,
    pub append_system: &'static str,
    pub yolo: &'static str,
    pub permission_mode: &'static str,
    pub continue_flag: &'static str,
    pub resume: &'static str,
    /// `None` ⇒ this CLI has no turn cap; aisdk warns and drops `--max-turns`.
    pub max_turns: Option<&'static str>,
    pub output: fn(Format) -> &'static [&'static str],
    pub tools: ToolStyle,
    pub prompt: PromptStyle,
    pub api_key_vars: &'static [&'static str],
    pub bin_env: &'static str,
    pub default_bin: &'static str,
    pub creds_path: &'static str,
    pub login_hint: &'static str,
    pub parse_json: fn(&str) -> Result<RunResult>,
    pub parse_event: fn(&Value) -> StreamEvent,
}

fn claude_output(f: Format) -> &'static [&'static str] {
    match f {
        Format::Text => &[],
        Format::Json => &["--output-format", "json"],
        Format::Stream => &[
            "--output-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
        ],
    }
}

fn grok_output(f: Format) -> &'static [&'static str] {
    match f {
        Format::Text => &["--output-format", "plain"],
        Format::Json => &["--output-format", "json"],
        Format::Stream => &["--output-format", "streaming-json"],
    }
}

static CLAUDE: Dialect = Dialect {
    print_flag: Some("--print"),
    model: "--model",
    system: "--system-prompt",
    append_system: "--append-system-prompt",
    yolo: "--dangerously-skip-permissions",
    permission_mode: "--permission-mode",
    continue_flag: "--continue",
    resume: "--resume",
    max_turns: None, // claude 2.1.x has no --max-turns (use --max-budget-usd)
    output: claude_output,
    tools: ToolStyle::Joined {
        allow: "--allowedTools",
        deny: "--disallowedTools",
    },
    prompt: PromptStyle::Positional,
    api_key_vars: &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
    bin_env: "AISDK_CLAUDE_BIN",
    default_bin: "claude",
    creds_path: ".claude/.credentials.json",
    login_hint: "run `claude`, or `claude setup-token` for CI",
    parse_json: event::parse_claude_json,
    parse_event: event::parse_claude_event,
};

static GROK: Dialect = Dialect {
    print_flag: None,
    model: "-m",
    system: "--system-prompt-override",
    append_system: "--rules",
    yolo: "--always-approve",
    permission_mode: "--permission-mode",
    continue_flag: "--continue",
    resume: "--resume",
    max_turns: Some("--max-turns"),
    output: grok_output,
    tools: ToolStyle::Repeated {
        allow: "--allow",
        deny: "--deny",
    },
    prompt: PromptStyle::Flag("--single"),
    api_key_vars: &["XAI_API_KEY", "GROK_CODE_XAI_API_KEY"],
    bin_env: "AISDK_GROK_BIN",
    default_bin: "grok",
    creds_path: ".grok/auth.json",
    login_hint: "run `grok` (browser / device-code OAuth)",
    parse_json: event::parse_grok_json,
    parse_event: event::parse_grok_event,
};

/// Append a `flag value` pair to the argv.
fn push_pair(a: &mut Vec<String>, flag: &str, val: &str) {
    a.push(flag.to_string());
    a.push(val.to_string());
}

/// Build a CLI's argv from any dialect — the single source of mapping logic.
fn build_argv(d: &Dialect, r: &Request) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();

    if let Some(f) = d.print_flag {
        a.push(f.into());
    }
    a.extend((d.output)(r.format).iter().map(|s| s.to_string()));

    if let Some(m) = &r.model {
        push_pair(&mut a, d.model, m);
    }
    if let Some(s) = &r.system {
        push_pair(&mut a, d.system, s);
    }
    if let Some(s) = &r.append_system {
        push_pair(&mut a, d.append_system, s);
    }
    if r.yolo {
        a.push(d.yolo.into());
    }
    if let Some(pm) = &r.permission_mode {
        push_pair(&mut a, d.permission_mode, pm);
    }
    if r.continue_session {
        a.push(d.continue_flag.into());
    }
    if let Some(id) = &r.resume {
        push_pair(&mut a, d.resume, id);
    }
    if let (Some(flag), Some(n)) = (d.max_turns, r.max_turns) {
        push_pair(&mut a, flag, &n.to_string());
    }

    match &d.tools {
        ToolStyle::Joined { allow, deny } => {
            if !r.allow.is_empty() {
                push_pair(&mut a, allow, &r.allow.join(","));
            }
            if !r.deny.is_empty() {
                push_pair(&mut a, deny, &r.deny.join(","));
            }
        }
        ToolStyle::Repeated { allow, deny } => {
            for v in &r.allow {
                push_pair(&mut a, allow, v);
            }
            for v in &r.deny {
                push_pair(&mut a, deny, v);
            }
        }
    }

    a.extend(r.passthrough.iter().cloned());

    match d.prompt {
        PromptStyle::Positional => a.push(r.prompt.clone()),
        PromptStyle::Flag(flag) => push_pair(&mut a, flag, &r.prompt),
    }
    a
}

impl Provider {
    pub fn from_alias(s: &str) -> Result<Provider> {
        match s.to_ascii_lowercase().as_str() {
            "claude" | "anthropic" | "cc" => Ok(Provider::Claude),
            "grok" | "xai" | "grok-build" | "gb" => Ok(Provider::Grok),
            other => bail!("unknown provider '{other}' (use: claude | grok)"),
        }
    }

    pub fn name(self) -> &'static str {
        self.dialect().default_bin
    }

    pub fn dialect(self) -> &'static Dialect {
        match self {
            Provider::Claude => &CLAUDE,
            Provider::Grok => &GROK,
        }
    }

    /// Binary to spawn; overridable via env for non-standard installs.
    pub fn binary(self) -> String {
        let d = self.dialect();
        std::env::var(d.bin_env).unwrap_or_else(|_| d.default_bin.into())
    }

    /// API-key env vars that route a CLI to metered billing.
    pub fn api_key_vars(self) -> &'static [&'static str] {
        self.dialect().api_key_vars
    }

    /// In subscription mode, remove API-key vars so the CLI falls back to OAuth.
    pub fn scrub_env(self, cmd: &mut Command, billing: Billing) {
        if billing == Billing::Subscription {
            for k in self.api_key_vars() {
                cmd.env_remove(k);
            }
        }
    }

    /// Capability gaps where a requested option can't be honored by this CLI.
    pub fn warnings(self, r: &Request) -> Vec<String> {
        let mut w = Vec::new();
        if self.dialect().max_turns.is_none() && r.max_turns.is_some() {
            w.push(format!(
                "{} has no --max-turns; ignoring it (cap spend with `-- --max-budget-usd <amount>`)",
                self.name()
            ));
        }
        w
    }

    pub fn argv(self, r: &Request) -> Vec<String> {
        build_argv(self.dialect(), r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(provider: Provider) -> Request {
        Request {
            provider,
            prompt: "hi".into(),
            ..Default::default()
        }
    }

    /// argv as &str for easy comparison.
    fn argv(p: Provider, r: &Request) -> Vec<String> {
        p.argv(r)
    }

    fn has_pair(a: &[String], flag: &str, val: &str) -> bool {
        a.windows(2).any(|w| w[0] == flag && w[1] == val)
    }

    #[test]
    fn claude_json_exact_argv() {
        let mut r = req(Provider::Claude);
        r.format = Format::Json;
        let a = argv(Provider::Claude, &r);
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        assert_eq!(a, ["--print", "--output-format", "json", "hi"]);
    }

    #[test]
    fn claude_tools_joined_yolo_and_positional_prompt() {
        let mut r = req(Provider::Claude);
        r.yolo = true;
        r.allow = vec!["Read".into(), "Bash".into()];
        r.deny = vec!["Edit".into()];
        let a = argv(Provider::Claude, &r);
        assert!(has_pair(&a, "--allowedTools", "Read,Bash"));
        assert!(has_pair(&a, "--disallowedTools", "Edit"));
        assert!(a.contains(&"--dangerously-skip-permissions".to_string()));
        assert_eq!(a.last().unwrap(), "hi"); // prompt is the trailing positional
    }

    #[test]
    fn claude_drops_max_turns_and_warns() {
        let mut r = req(Provider::Claude);
        r.max_turns = Some(5);
        assert!(!argv(Provider::Claude, &r).contains(&"--max-turns".to_string()));
        assert!(!Provider::Claude.warnings(&r).is_empty());
    }

    #[test]
    fn grok_stream_uses_single_flag_for_prompt() {
        let mut r = req(Provider::Grok);
        r.format = Format::Stream;
        let a = argv(Provider::Grok, &r);
        assert_eq!(&a[0..2], &["--output-format", "streaming-json"]);
        assert!(has_pair(&a, "--single", "hi"));
    }

    #[test]
    fn grok_tools_repeated_and_keeps_max_turns() {
        let mut r = req(Provider::Grok);
        r.allow = vec!["Read".into(), "Bash".into()];
        r.max_turns = Some(3);
        let a = argv(Provider::Grok, &r);
        assert_eq!(a.iter().filter(|x| *x == "--allow").count(), 2);
        assert!(has_pair(&a, "--max-turns", "3"));
        assert!(Provider::Grok.warnings(&r).is_empty());
    }

    #[test]
    fn key_vars_and_names_come_from_dialect() {
        assert_eq!(
            Provider::Claude.api_key_vars(),
            &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]
        );
        assert_eq!(
            Provider::Grok.api_key_vars(),
            &["XAI_API_KEY", "GROK_CODE_XAI_API_KEY"]
        );
        assert_eq!(Provider::Claude.name(), "claude");
        assert_eq!(Provider::Grok.name(), "grok");
    }
}
