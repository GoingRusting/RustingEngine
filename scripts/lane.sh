#!/usr/bin/env bash
# Parallel agent sessions, one git worktree ("lane") each. See AGENTS.md.
#   scripts/lane.sh <name>          create ../RustingEngine-<name> on lane/<name>
#   scripts/lane.sh claim "<item>"  claim a roadmap item for this lane
#   scripts/lane.sh done "<item>"   drop a claim
#   scripts/lane.sh claims          list claims
set -euo pipefail

# Shared by every worktree of this repository, and never committed.
claims="$(git rev-parse --path-format=absolute --git-common-dir)/lane-claims"
lane="$(git branch --show-current)"
touch "$claims"

case "${1:-}" in
"" | -h | --help)
    sed -n '2,6p' "$0"
    ;;
claim)
    item="${2:?item name}"
    (
        flock 9
        holder="$(awk -F'\t' -v item="$item" '$2 == item { print $1 }' "$claims")"
        if [[ -n "$holder" && "$holder" != "$lane" ]]; then
            echo "taken by $holder: $item" >&2
            exit 1
        fi
        [[ -n "$holder" ]] || printf '%s\t%s\n' "$lane" "$item" >>"$claims"
        echo "claimed by $lane: $item"
    ) 9>"$claims.lock"
    ;;
done)
    item="${2:?item name}"
    (
        flock 9
        awk -F'\t' -v item="$item" '$2 != item' "$claims" >"$claims.new"
        mv "$claims.new" "$claims"
    ) 9>"$claims.lock"
    ;;
claims)
    cat "$claims"
    ;;
*)
    name="$1"
    main="$(git rev-parse --path-format=absolute --git-common-dir)/.."
    main="$(cd -- "$main" && pwd)"
    path="$(dirname -- "$main")/RustingEngine-$name"
    git -C "$main" worktree add "$path" -b "lane/$name" main
    # AGENTS.md is git-ignored, so link the one source of the rules.
    ln -s "$main/AGENTS.md" "$path/AGENTS.md"
    echo "lane ready: cd $path && claude"
    ;;
esac
