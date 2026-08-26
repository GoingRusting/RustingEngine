#!/usr/bin/env bash
set -euo pipefail

archive="${1:-rusting-engine-linux-x86_64.tar.gz}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
engine_dir="$(cd -- "$script_dir/.." && pwd)"
package_dir="$(mktemp -d)"
trap 'rm -rf -- "$package_dir"' EXIT

mkdir -p "$engine_dir/dist" "$package_dir/RustingEngine"
cp "$engine_dir/target/release/editor" "$package_dir/RustingEngine/"
cp "$engine_dir/target/release/user_main" "$package_dir/RustingEngine/"
cp "$engine_dir/target/release/cook_scene" "$package_dir/RustingEngine/"
cp "$engine_dir/README.md" "$package_dir/RustingEngine/"
mkdir -p "$package_dir/RustingEngine/docs/images"
cp "$engine_dir/docs/images/spaceCubes.jpg" "$package_dir/RustingEngine/docs/images/"
cp "$engine_dir/CONTRIBUTING.md" "$package_dir/RustingEngine/"
cp "$engine_dir/architecture.md" "$package_dir/RustingEngine/"
cp "$engine_dir/roadmap.md" "$package_dir/RustingEngine/"
cp "$engine_dir/RELEASE.md" "$package_dir/RustingEngine/"
cp "$engine_dir/editor_gui.md" "$package_dir/RustingEngine/"
cp "$engine_dir/CHANGELOG.md" "$package_dir/RustingEngine/"
cp "$engine_dir/LICENSE.md" "$package_dir/RustingEngine/"
tar -C "$package_dir" -czf "$engine_dir/dist/$archive" RustingEngine

echo "Created dist/$archive"
