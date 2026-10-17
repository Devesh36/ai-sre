#!/usr/bin/env bash
# Source this file to activate the workspace-local Rust installation.
export CARGO_HOME=/workspace/.toolchains/cargo
export RUSTUP_HOME=/workspace/.toolchains/rustup
export PATH="$CARGO_HOME/bin:$PATH"
