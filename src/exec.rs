//! Transport: spawn the child CLI, wire stdio, and hand its output to `render`.
//! All provider-specific behavior comes from `r.provider` / its dialect.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::provider::{Format, Request};
use crate::render::{self, StreamSink};
use crate::style::Style;

pub fn run(r: &Request) -> Result<i32> {
    let bin = r.provider.binary();
    let args = r.provider.argv(r);
    let est = Style::for_stderr();

    for w in r.provider.warnings(r) {
        eprintln!("{}", est.dim(&format!("⚠ aisdk: {w}")));
    }

    let mut cmd = Command::new(&bin);
    cmd.args(&args);
    if let Some(cwd) = &r.cwd {
        cmd.current_dir(cwd);
    }
    r.provider.scrub_env(&mut cmd, r.billing);
    // Prompt is passed as an argument, so the child never needs stdin.
    // (Claude otherwise blocks ~3s waiting for piped stdin.)
    cmd.stdin(Stdio::null());

    if r.verbose || r.dry_run {
        for line in render::plan(est, r, &bin, &args) {
            eprintln!("{line}");
        }
    }
    if r.dry_run {
        return Ok(0);
    }

    match r.format {
        Format::Text => {
            cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
            Ok(cmd.status().with_context(|| spawn_err(&bin))?.code().unwrap_or(1))
        }
        Format::Json => {
            // Keep stderr separate so the CLI's logs don't corrupt the JSON on stdout.
            cmd.stdout(Stdio::piped()).stderr(Stdio::inherit());
            let out = cmd.output().with_context(|| spawn_err(&bin))?;
            let so = String::from_utf8_lossy(&out.stdout);
            match (r.provider.dialect().parse_json)(&so) {
                Ok(res) => println!("{}", serde_json::to_string_pretty(&res)?),
                Err(e) => {
                    eprintln!(
                        "aisdk: couldn't parse {} JSON ({e}); raw output follows:",
                        r.provider.name()
                    );
                    print!("{so}");
                }
            }
            Ok(out.status.code().unwrap_or(1))
        }
        Format::Stream => {
            cmd.stdout(Stdio::piped()).stderr(Stdio::inherit());
            let mut child = cmd.spawn().with_context(|| spawn_err(&bin))?;
            let so = child.stdout.take().expect("piped stdout");
            let parse = r.provider.dialect().parse_event;
            let mut sink = StreamSink::new(r.provider.name(), r.verbose);
            for line in BufReader::new(so).lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    sink.handle(parse(&v));
                }
            }
            let code = child.wait()?.code().unwrap_or(1);
            sink.finish();
            Ok(code)
        }
    }
}

fn spawn_err(bin: &str) -> String {
    format!("could not run `{bin}` — is it installed and on PATH? Try `aisdk doctor`")
}
