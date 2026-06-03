use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use crate::provider::{Billing, Format, Provider, Request};

/// One entry point for Claude Code (`claude`) and Grok Build (`grok`).
#[derive(Parser, Debug)]
#[command(
    name = "aisdk",
    version,
    about = "One entry point for Claude Code (claude) and Grok Build (grok) — using your subscriptions, not metered API.",
    after_help = "EXAMPLES:\n  aisdk --use=grok --prompt \"Summarise this email\"\n  aisdk --use=claude:opus -p \"Explain this repo\" --format stream\n  cat email.txt | aisdk --use=grok -p - --format json\n  aisdk doctor"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Provider and optional model: claude | grok, with :model (e.g. grok:grok-build-0.1, claude:opus)
    #[arg(long = "use", value_name = "PROVIDER[:MODEL]")]
    pub use_spec: Option<String>,

    /// Prompt text, or "-" to read from stdin
    #[arg(short, long, value_name = "TEXT")]
    pub prompt: Option<String>,

    /// Read the prompt from a file
    #[arg(long = "prompt-file", value_name = "PATH")]
    pub prompt_file: Option<PathBuf>,

    /// Override the model (takes precedence over the :model in --use)
    #[arg(long, value_name = "NAME")]
    pub model: Option<String>,

    /// System prompt (replaces the default)
    #[arg(long, value_name = "TEXT")]
    pub system: Option<String>,

    /// Extra instructions appended to the default system prompt
    #[arg(long = "append-system", value_name = "TEXT")]
    pub append_system: Option<String>,

    /// Working directory for the agent
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Billing path: `subscription` (default) strips API-key env vars so the CLI uses OAuth
    #[arg(long, value_enum, default_value = "subscription")]
    pub billing: Billing,

    /// Output format: text (default) | json (unified result) | stream (live, unified)
    #[arg(long, value_enum, default_value = "text")]
    pub format: Format,

    /// Auto-approve all tool use (claude: --dangerously-skip-permissions, grok: --always-approve)
    #[arg(long)]
    pub yolo: bool,

    /// Permission mode: default|acceptEdits|auto|dontAsk|bypassPermissions|plan
    #[arg(long = "permission-mode", value_name = "MODE")]
    pub permission_mode: Option<String>,

    /// Continue the most recent session in the working directory
    #[arg(long = "continue")]
    pub continue_session: bool,

    /// Resume a session by id
    #[arg(long, value_name = "ID")]
    pub resume: Option<String>,

    /// Maximum number of agent turns
    #[arg(long = "max-turns", value_name = "N")]
    pub max_turns: Option<u32>,

    /// Allow a tool (repeatable). e.g. --allow Read --allow "Bash(git *)"
    #[arg(long, value_name = "TOOL")]
    pub allow: Vec<String>,

    /// Deny a tool (repeatable)
    #[arg(long, value_name = "TOOL")]
    pub deny: Vec<String>,

    /// Print the resolved child command + env changes, then exit without running
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Log the spawned command (and reasoning, in stream mode) to stderr
    #[arg(short, long)]
    pub verbose: bool,

    /// Mute the underlying CLI's own stderr logging (e.g. Grok's MCP startup noise)
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Everything after `--` is forwarded verbatim to the underlying CLI
    #[arg(last = true, value_name = "-- ARGS")]
    pub passthrough: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Diagnose installed CLIs, auth paths, and billing footguns
    Doctor,
}

impl Cli {
    pub fn into_request(self) -> Result<Request> {
        let spec = self
            .use_spec
            .context("missing --use (e.g. --use=grok or --use=claude:opus). Run `aisdk doctor` to check setup")?;
        let (provider, use_model) = parse_use(&spec)?;
        Ok(Request {
            provider,
            model: self.model.or(use_model),
            prompt: resolve_prompt(self.prompt, self.prompt_file)?,
            system: self.system,
            append_system: self.append_system,
            cwd: self.cwd,
            billing: self.billing,
            format: self.format,
            yolo: self.yolo,
            permission_mode: self.permission_mode,
            continue_session: self.continue_session,
            resume: self.resume,
            max_turns: self.max_turns,
            allow: self.allow,
            deny: self.deny,
            passthrough: self.passthrough,
            verbose: self.verbose,
            quiet: self.quiet,
            dry_run: self.dry_run,
        })
    }
}

/// Parse `provider[:model]` into a provider and optional model.
fn parse_use(spec: &str) -> Result<(Provider, Option<String>)> {
    let (p, m) = match spec.split_once(':') {
        Some((p, m)) => (p, Some(m.trim().to_string()).filter(|s| !s.is_empty())),
        None => (spec, None),
    };
    Ok((Provider::from_alias(p.trim())?, m))
}

fn resolve_prompt(prompt: Option<String>, file: Option<PathBuf>) -> Result<String> {
    use std::io::Read;
    if let Some(f) = file {
        return std::fs::read_to_string(&f)
            .with_context(|| format!("reading --prompt-file {}", f.display()));
    }
    match prompt.as_deref() {
        Some("-") => {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .context("reading prompt from stdin")?;
            Ok(s)
        }
        Some(p) => Ok(p.to_string()),
        None => bail!(r#"no prompt — pass --prompt "...", --prompt-file PATH, or --prompt - (stdin)"#),
    }
}
