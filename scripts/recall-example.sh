#!/usr/bin/env bash
# Two real offline investigations in an isolated, retained synthetic workspace.
set -euo pipefail
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
binary=${AI_SRE_BIN:-"$repo_root/target/debug/ai-sre"}
if [[ ! -x "$binary" ]]; then
    printf 'Build first with cargo build --locked, or set AI_SRE_BIN.\n' >&2
    exit 1
fi
mkdir -p "$repo_root/.ai-sre"
demo_dir=$(mktemp -d "$repo_root/.ai-sre/recall-example.XXXXXX")
cp "$repo_root/examples/evidence.json" "$demo_dir/evidence.json"
mkdir "$demo_dir/runbooks"
cp "$repo_root/examples/runbooks/checkout.md" "$demo_dir/runbooks/checkout.md"
cat > "$demo_dir/config.toml" <<'CONFIG'
database = "incidents.db"
evidence_file = "evidence.json"
runbooks = "runbooks"
CONFIG
"$binary" --config "$demo_dir/config.toml" ask 'Previous checkout incident: inspect memory and inventory' --service checkout-api --json > "$demo_dir/past.json"
past_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$demo_dir/past.json")
printf 'Marking the synthetic past case resolved; this does not verify a particular fix.\n'
"$binary" --config "$demo_dir/config.toml" resolve "$past_id" > "$demo_dir/resolution.txt"
"$binary" --config "$demo_dir/config.toml" show "$past_id" > "$demo_dir/past.json"
"$binary" --config "$demo_dir/config.toml" ask 'Checkout errors returned: which past observations match?' --service checkout-api --json > "$demo_dir/current.json"
current_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$demo_dir/current.json")
NO_COLOR=1 "$binary" --config "$demo_dir/config.toml" recall "$current_id" | tee "$demo_dir/recall.txt"
"$binary" --config "$demo_dir/config.toml" recall "$current_id" --json > "$demo_dir/recall.json"
printf '\nSaved both cases and the recall explanation to:\n%s\n' "$demo_dir"
printf '\nOpen the workspace (select Recall on the newest case):\n'
printf '%q --config %q serve\n' "$binary" "$demo_dir/config.toml"
