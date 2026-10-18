# Terminal workspace

Install the Rust executable from this checkout, then launch it:

```sh
cargo install --path . --locked
ai-sre repl
```

Ensure Cargo's bin directory is on your PATH (normally `$HOME/.cargo/bin`). In the managed cloud checkout, activate the workspace toolchain first with `source scripts/cloud-env.sh`; its Cargo bin directory is `/workspace/.toolchains/cargo/bin`.

Running `ai-sre` without a subcommand opens the interactive terminal when stdin is a terminal. `ai-sre repl` explicitly starts the REPL; `ai-sre cli`, `ai-sre shell`, and `ai-sre chat` are aliases for the same workspace. Use `ai-sre repl --help` for launch options and `/help` inside the REPL for interactive commands. Use `--config PATH` to choose settings; otherwise it loads `sre.toml` from the current directory when present. With no model configured, the welcome panel clearly labels offline triage. It does not pretend to provide general model chat.

```sh
ai-sre --config examples/demo.toml repl --service checkout-api
ai-sre cli --service checkout-api
ai-sre --config sre.toml ask "Why are requests timing out?" --service checkout-api
ai-sre --config sre.toml ask "Check dependencies" --service checkout-api --json
printf '%s' 'Check service health' | ai-sre ask - --service checkout-api --json
```

The terminal uses scrollback rather than an alternate-screen dashboard. Reports have a status banner, summary, findings with evidence references, suggested next steps, gaps, and usage. Text wraps to terminal width. During an investigation, a progress indicator shows elapsed time and the cancellation shortcut; it does not simulate token streaming.

The default accent is warm amber (`#e0a672`), shared with the dashboard. True-color terminals use the exact color; 256-color and basic ANSI terminals use compatible fallbacks. The CLI inherits your terminal's background and normal text colors. To match the README preview, set your terminal background to `#25221e` and foreground to `#e6dccd`. `NO_COLOR` keeps plain output available.

## Editing and navigation

- Tab completes slash commands; press Tab again to list ambiguous matches.
- Up/Down recall recent questions. Ctrl+R searches that history.
- Left/Right, Home/End, Ctrl+A/Ctrl+E, and standard Emacs editing keys edit the prompt.
- Ctrl+C clears pending input. During an investigation it requests cancellation and retains completed evidence.
- Ctrl+D exits from an empty prompt. `/exit` also quits.
- Pasted multiline questions are accepted subject to the existing 4,000-byte question limit.

The latest 200 questions for the selected service populate editor history. Only redacted questions are retained in editor history and SQLite; no separate raw line-history file is written. Redaction is best effort. The text you type is visible on your terminal, so avoid entering credentials. `/clear-history` clears both the editor's recall list and persistent history for that service.

## Commands

| Command | Effect |
| --- | --- |
| `/help` | Commands, keyboard shortcuts, and a sample question |
| `/service NAME` | Choose a service, load its history, and start a fresh conversation |
| `/new` | Start a fresh conversation for the current service |
| `/sessions` | List saved investigations with full IDs |
| `/resume ID` | Resume an investigation and switch to its service/history |
| `/status` | Current title, service, status, and ID |
| `/cost` | Tool/model calls, tokens, and estimated cost when configured |
| `/evidence` | Evidence IDs, sources, timestamps, and summaries |
| `/trace` | Recorded execution phases and elapsed times |
| `/compact` | Compact future model context while retaining evidence |
| `/integrations` | Discover and list available configured tools |
| `/history` | Show the latest 20 questions for this service |
| `/clear-history` | Clear this service's saved questions and editor recall |
| `/exit` | Exit the terminal workspace |

Invalid commands, invalid resume IDs, and failed investigations return control to the prompt. Resolved investigations retain their existing restriction against restarting; use `/new` for a new investigation.

## Plain output and automation

`NO_COLOR` disables colors and the animated progress indicator. `TERM=dumb` or redirected terminal input/output uses a plain line-input fallback. JSON output always bypasses banners, report styling, and progress output. A no-argument invocation with non-terminal stdin prints help without opening a database; scripted questions use `ask -` or explicit `shell`.

Report and metadata rendering removes terminal control sequences, including OSC clipboard/title escapes and bidirectional text controls. JSON exports preserve their structured data representation.

Run `cargo test --locked --test terminal` for formatting, completion, script-output, and shell regression checks. On Linux/macOS, `python3 scripts/terminal-smoke.py target/debug/ai-sre` exercises a real pseudo-terminal, including resize, history, cancellation, and terminal restoration. Python is needed only for this developer test, not to run the product. The editor library supports Windows, but Windows terminal behavior has not been validated here.
