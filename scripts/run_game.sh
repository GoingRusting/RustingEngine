#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$script_dir/.."
scene="${1:-testGame/build/main.rscene.bin}"
exec mangohud cargo run --no-default-features --features window --bin game -- "$scene"
