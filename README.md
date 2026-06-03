# aisdk

One entry point for two terminal coding agents — **Claude Code** (`claude`) and
**Grok Build** (`grok`) — using your **subscriptions**, not metered API credits.

```sh
aisdk --use=grok   --prompt "Summarise this email's contents"
aisdk --use=claude --prompt "Explain this repo" --format stream
```

Both tools are the same shape (headless `-p`, streaming JSON, models, sub-agents,
MCP, two-path auth). `aisdk` is a thin Rust orchestrator that **normalizes one flag
dialect** to each CLI, **parses their different JSON outputs into one shape**, and —
most importantly — **keeps you on your subscription** by stripping API-key env vars
from the child process so the CLI uses its cached OAuth login.

## Why the subscription part matters

Each CLI bills two ways, and the danger is silently falling through to the metered API:

| | Subscription (default here) | Metered API |
|---|---|---|
| **claude** | OAuth login / `claude setup-token` | `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` |
| **grok** | OAuth (browser / device-code) | `XAI_API_KEY`, `GROK_CODE_XAI_API_KEY` |

If `ANTHROPIC_API_KEY` is in your shell, plain `claude` bills the API even when you're
subscribed. In `--billing subscription` (the default) `aisdk` removes those vars from
the child, so the CLI authenticates with your subscription:

```
plain (key present):  "apiKeySource":"ANTHROPIC_API_KEY"   # billed to the API
via aisdk (scrubbed):  "apiKeySource":"none"               # uses the subscription
```

Run `aisdk doctor` to see which path each CLI is on and get warned about the footgun.

## Build

```sh
cargo build --release      # binary at target/release/aisdk
```

Requires `claude` and `grok` already installed and logged in (`aisdk doctor` checks both).

## Usage

```
aisdk --use=<provider>[:<model>] --prompt <text> [options]
aisdk doctor
```

| Flag | Meaning | claude | grok |
|---|---|---|---|
| `--use=p[:model]` | provider + optional model | — | — |
| `-p, --prompt <T>` | prompt (`-` = stdin) | positional | `--single` |
| `--prompt-file <P>` | prompt from file | (read by aisdk) | (read by aisdk) |
| `--model <N>` | override model | `--model` | `-m` |
| `--system <T>` | replace system prompt | `--system-prompt` | `--system-prompt-override` |
| `--append-system <T>` | append to system prompt | `--append-system-prompt` | `--rules` |
| `--cwd <D>` | working directory | `current_dir` | `--cwd` |
| `--format <F>` | `text` \| `json` \| `stream` | `--output-format` | `--output-format` |
| `--yolo` | auto-approve all tools | `--dangerously-skip-permissions` | `--always-approve` |
| `--permission-mode <M>` | permission mode | `--permission-mode` | `--permission-mode` |
| `--continue` / `--resume <ID>` | session control | `-c` / `-r` | `-c` / `-r` |
| `--max-turns <N>` | turn cap | *(unsupported → warns)* | `--max-turns` |
| `--allow <T>` / `--deny <T>` | tool rules (repeatable) | `--allowedTools` / `--disallowedTools` | `--allow` / `--deny` |
| `--billing <B>` | `subscription` (default) \| `api` | env scrub | env scrub |
| `--dry-run` | print resolved command, don't run | — | — |
| `-v, --verbose` | log command + reasoning to stderr | — | — |
| `-q, --quiet` | mute the underlying CLI's own stderr logging | — | — |
| `-- <ARGS>` | forward raw args to the underlying CLI | ✓ | ✓ |

Anything not yet mapped is reachable with the `--` escape hatch:

```sh
aisdk --use=grok -p "refactor" -- --best-of-n 3 --check
aisdk --use=claude -p "audit" -- --max-budget-usd 2.00
```

Override binary locations with `AISDK_CLAUDE_BIN` / `AISDK_GROK_BIN`.

## Output formats

- **`text`** (default): the child's stdout is inherited verbatim — fast, colors intact.
- **`json`**: one provider-agnostic result object on stdout (CLI logs stay on stderr):

  ```json
  // claude reports cost/usage/model; grok reports reasoning
  { "provider": "claude", "text": "...", "session_id": "...", "stop_reason": "end_turn",
    "is_error": false, "model": "claude-opus-4-8", "cost_usd": 0.06,
    "input_tokens": 7217, "output_tokens": 4 }

  { "provider": "grok", "text": "...", "reasoning": "...", "session_id": "...",
    "stop_reason": "EndTurn", "is_error": false }
  ```

- **`stream`**: the answer streams to **stdout** as it arrives; status, reasoning
  (with `-v`), tool calls, and a final `── provider · model · $cost · tokens · session`
  summary go to **stderr** — so `aisdk ... --format stream > answer.txt` captures just the answer.

Fields Grok doesn't report (cost, token counts, model) are omitted rather than faked.

## Roadmap

- **v0.2** — config file (`~/.config/aisdk/config.toml`): default provider/model/billing; `aisdk login`, `aisdk providers`.
- **v0.3** — sub-agent fan-out via `--agents '{json}'` (both support it) and Grok's `--best-of-n`; cross-provider orchestration (draft on Grok → review on Claude).
- **v0.4** — MCP passthrough (`--mcp-config` for claude / `.grok` config for grok), wiring existing MCP servers (e.g. Gmail/Stalwart) so "summarise this email" can fetch the email.
- **v0.5** — ACP mode: expose `aisdk` itself as an agent server.

## Layout

```
src/
  main.rs      entry: parse → dispatch → propagate exit code
  cli.rs       clap args; parse --use=provider[:model]
  provider.rs  Provider enum: argv mapping, env scrubbing, capability warnings
  event.rs     unified RunResult + StreamEvent; claude/grok json + stream parsers
  exec.rs      spawn child, stdio wiring, renderers for text/json/stream
  doctor.rs    diagnostics: binaries, auth paths, billing footguns
```
