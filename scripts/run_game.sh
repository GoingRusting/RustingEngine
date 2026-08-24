#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$script_dir/.."
scene="${1:-assets/scenes/main.rscene.bin}"
exec cargo run --release --no-default-features --features window --bin game -- "$scene"
