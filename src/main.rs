mod cli;
mod doctor;
mod event;
mod exec;
mod provider;
mod render;
mod style;

use clap::Parser;

fn main() {
    let code = match run() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aisdk: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> anyhow::Result<i32> {
    let cli = cli::Cli::parse();
    if let Some(cli::Command::Doctor) = cli.command {
        return doctor::run();
    }
    let req = cli.into_request()?;
    exec::run(&req)
}
