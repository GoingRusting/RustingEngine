#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$script_dir/.."
source_scene="${1:-assets/scenes/main.rscene}"
compiled_scene="${2:-assets/scenes/main.rscene.bin}"
exec cargo run --bin cook_scene -- "$source_scene" "$compiled_scene"
