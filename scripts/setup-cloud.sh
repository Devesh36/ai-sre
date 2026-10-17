#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
source scripts/cloud-env.sh
if [[ ! -x "$CARGO_HOME/bin/rustup" ]]; then
  installer=$(mktemp /tmp/ai-sre-rustup.XXXXXX)
  trap 'rm -f "$installer"' EXIT
  curl --fail --silent --show-error --proto '=https' --tlsv1.2 https://sh.rustup.rs -o "$installer"
  sh "$installer" -y --no-modify-path --profile minimal --default-toolchain 1.90.0
fi
rustup toolchain install 1.90.0 --profile minimal --component rustfmt --component clippy
cargo build --locked --jobs 4
cargo test --locked --jobs 4
cargo run --locked -- eval
cargo install --path . --locked --offline --jobs 4
