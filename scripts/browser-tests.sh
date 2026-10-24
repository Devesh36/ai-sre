#!/usr/bin/env bash
# Isolated synthetic workspace; never runs against the user's incident database.
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
binary="${AI_SRE_BIN:-$root/target/debug/ai-sre}"
workspace="$(mktemp -d)"
server_pid=''
cleanup() {
  if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi
  rm -rf "$workspace"
}
trap cleanup EXIT
mkdir -p .ai-sre
python3 - "$root" "$workspace" <<'PY'
import json, pathlib, sys
root, workspace = map(pathlib.Path, sys.argv[1:])
(workspace / 'config.toml').write_text('database = "incidents.db"\nevidence_file = ' + json.dumps(str(root / 'examples/evidence.json')) + '\nrunbooks = ' + json.dumps(str(root / 'examples/runbooks')) + '\n')
PY
"$binary" --config "$workspace/config.toml" ask 'Browser acceptance seed' --service checkout-api --json > "$workspace/seed.json"
export SRE_TEST_URL="http://127.0.0.1:${AI_SRE_BROWSER_PORT:-18080}"
env -u SRE_API_TOKEN "$binary" --config "$workspace/config.toml" serve --bind "127.0.0.1:${AI_SRE_BROWSER_PORT:-18080}" > "$workspace/server.log" 2>&1 &
server_pid=$!
# Confirm this process is listening before tests can touch any existing server.
if ! python3 - "$workspace/server.log" "$server_pid" <<'PY'
import os, pathlib, sys, time
log = pathlib.Path(sys.argv[1])
for _ in range(100):
    os.kill(int(sys.argv[2]), 0)
    if 'listening' in log.read_text():
        break
    time.sleep(0.1)
else:
    raise SystemExit('Browser test server did not start')
PY
then cat "$workspace/server.log"; exit 1; fi
node scripts/dashboard-state-smoke.cjs
node scripts/browser-smoke.cjs
node scripts/review-smoke.cjs
node scripts/operations-smoke.cjs
