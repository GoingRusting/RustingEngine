#!/usr/bin/env bash
# Copies each sample's README.md and src/main.rs into docs/samples/<name>/.
# The samples are their own Cargo packages, so `cargo publish` leaves them out of
# the engine tarball; `rusting docs` reads these copies instead.
# Run after changing a sample. The docs tests fail when a copy is out of date.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$script_dir/.."

for dir in samples/*/; do
    name="$(basename -- "$dir")"
    [[ -f "$dir/README.md" && -f "$dir/src/main.rs" ]] || continue
    mkdir -p "docs/samples/$name"
    cp "$dir/README.md" "docs/samples/$name/README.md"
    cp "$dir/src/main.rs" "docs/samples/$name/main.rs"
done
