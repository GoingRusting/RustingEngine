#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$script_dir/.."
project="${1:-testGame}"
exec cargo run --release --manifest-path "$project/Cargo.toml"
