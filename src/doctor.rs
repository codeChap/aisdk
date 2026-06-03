//! `aisdk doctor` — iterate providers (via their dialects) and report install
//! status, auth path, and the API-key-overrides-subscription footgun.

use std::path::Path;

use anyhow::Result;

use crate::provider::Provider;
use crate::style::Style;

const PROVIDERS: [Provider; 2] = [Provider::Claude, Provider::Grok];

pub fn run() -> Result<i32> {
    let st = Style::for_stdout();
    println!("aisdk doctor\n");

    let mut ok = true;
    for p in PROVIDERS {
        ok &= check(st, p);
        println!();
    }

    println!("Subscription billing (the default) uses each CLI's OAuth login and strips the API-key");
    println!("vars above from the child process. Log in once if subscription creds are missing:");
    for p in PROVIDERS {
        let d = p.dialect();
        println!("  {}: {}  → ~/{}", p.name(), d.login_hint, d.creds_path);
    }
    Ok(if ok { 0 } else { 1 })
}

fn check(st: Style, p: Provider) -> bool {
    let bin = p.binary();
    let d = p.dialect();

    match version(&bin) {
        Some(v) => println!("{} {}: {v}", mark(st, true), p.name()),
        None => {
            println!("{} {}: not found on PATH", mark(st, false), p.name());
            return false;
        }
    }

    let (path, exists) = home_join(d.creds_path);
    let creds = if exists { mark(st, true) } else { "—".to_string() };
    println!("   subscription creds: {creds} {path}");

    let set: Vec<&str> = d.api_key_vars.iter().copied().filter(|v| env_set(v)).collect();
    if set.is_empty() {
        println!("   api-key env: none set");
    } else {
        println!("   api-key env: {} {}", warn(st), set.join(", "));
    }

    if exists && !set.is_empty() {
        println!(
            "   {} {} set → plain `{}` bills the API; aisdk (subscription mode) scrubs it and uses your subscription",
            warn(st),
            set.join(" / "),
            p.name()
        );
    }
    if !exists && set.is_empty() {
        println!(
            "   {} no subscription creds and no API key — `{}` cannot authenticate (log in or set a key)",
            warn(st),
            p.name()
        );
        return false;
    }
    true
}

fn version(bin: &str) -> Option<String> {
    let o = std::process::Command::new(bin).arg("--version").output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn home_join(rel: &str) -> (String, bool) {
    let p = Path::new(&std::env::var("HOME").unwrap_or_default()).join(rel);
    (p.display().to_string(), p.exists())
}

fn env_set(v: &str) -> bool {
    std::env::var(v).map(|s| !s.is_empty()).unwrap_or(false)
}

fn mark(st: Style, ok: bool) -> String {
    if !st.color() {
        return if ok { "[ok]" } else { "[!!]" }.to_string();
    }
    if ok {
        st.green("✓")
    } else {
        st.red("✗")
    }
}

fn warn(st: Style) -> String {
    if st.color() {
        st.yellow("⚠")
    } else {
        "[warn]".to_string()
    }
}
