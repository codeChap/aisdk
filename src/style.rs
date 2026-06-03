//! One place for terminal styling. Build a `Style` once — it decides color from
//! whether the target stream is a TTY — and reuse it, instead of re-checking
//! `is_terminal()` on every write.

use std::io::IsTerminal;

#[derive(Copy, Clone)]
pub struct Style {
    color: bool,
}

impl Style {
    /// Color iff stderr is a terminal (decorations/summaries go to stderr).
    pub fn for_stderr() -> Self {
        Self {
            color: std::io::stderr().is_terminal(),
        }
    }

    /// Color iff stdout is a terminal (doctor prints to stdout).
    pub fn for_stdout() -> Self {
        Self {
            color: std::io::stdout().is_terminal(),
        }
    }

    pub fn color(self) -> bool {
        self.color
    }

    fn wrap(self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    pub fn dim(self, s: &str) -> String {
        self.wrap("2", s)
    }
    pub fn green(self, s: &str) -> String {
        self.wrap("32", s)
    }
    pub fn red(self, s: &str) -> String {
        self.wrap("31", s)
    }
    pub fn yellow(self, s: &str) -> String {
        self.wrap("33", s)
    }
}
