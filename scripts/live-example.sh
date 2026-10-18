#!/usr/bin/env bash
# Run a real, credential-free REPL session against copied synthetic fixtures.
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
binary=${AI_SRE_BIN:-"$repo_root/target/debug/ai-sre"}
if [[ ! -x "$binary" ]]; then
    printf 'Build first with cargo build --locked, or set AI_SRE_BIN to the executable path.\n' >&2
    exit 1
fi

mkdir -p "$repo_root/.ai-sre"
demo_dir=$(mktemp -d "$repo_root/.ai-sre/live-example.XXXXXX")
cp "$repo_root/examples/evidence.json" "$demo_dir/evidence.json"
mkdir "$demo_dir/runbooks"
cp "$repo_root/examples/runbooks/checkout.md" "$demo_dir/runbooks/checkout.md"
cat > "$demo_dir/config.toml" <<'CONFIG'
database = "incidents.db"
evidence_file = "evidence.json"
runbooks = "runbooks"
CONFIG
cat > "$demo_dir/commands.txt" <<'COMMANDS'
/integrations
Why is checkout failing after the latest deploy?
/evidence
Recheck memory and dependency evidence before deciding on a rollback.
/status
/cost
/trace
/sessions
/exit
COMMANDS

printf 'Running the real REPL with synthetic evidence and no model.\n'
printf 'Inputs: %s/commands.txt\n\n' "$demo_dir"
NO_COLOR=1 "$binary" --config "$demo_dir/config.toml" repl --service checkout-api \
    < "$demo_dir/commands.txt" | tee "$demo_dir/session.txt"
"$binary" --config "$demo_dir/config.toml" sessions > "$demo_dir/incidents.json"
printf '\nSaved transcript, input commands, JSON export, and database to:\n%s\n' "$demo_dir"
printf '\nContinue interactively (then use /sessions and /resume ID):\n'
printf '%q --config %q repl --service checkout-api\n' "$binary" "$demo_dir/config.toml"
