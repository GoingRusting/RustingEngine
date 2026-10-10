# RustingEngine Release Guide

## Release notes

Release notes are the `CHANGELOG.md` entries contributors add under
`## [Unreleased]` (see `CONTRIBUTING.md`). To cut a release:

1. Pick the version. New features or a `Changed` line that breaks saved
   scenes, game code or the wire protocol raise the minor version (2.3.0
   changed the protocol of 2.2.0); a release with only fixes raises the
   patch version.
2. Rename `## [Unreleased]` to `## [x.y.z] - YYYY-MM-DD` and add a fresh,
   empty `## [Unreleased]` above it.
3. Under the new heading, write one sentence saying what the release is
   for, such as the game or feedback that drove it.
4. Keep the sections in this order and drop empty ones: `Added`,
   `Changed`, `Fixed`, `Performance`, `Known limits`. Merge duplicate lines
   and remove lines for work that was reverted before the release.
5. End the entry with a `---` line, like the earlier ones.
6. Paste the entry into the GitHub Release description when publishing.

## Before tagging

1. Update the version in `Cargo.toml` to the one in `CHANGELOG.md`.
2. Run the complete local gate:

```bash
./scripts/check_release.sh
```

3. Run `./scripts/run_editor.sh` and check project create/open, scene editing,
   Cargo Check, Build & Run, and Export Game on a Vulkan-capable machine.
4. Confirm `git status` contains only the intended release changes, then commit
   and push them. Wait for the CI workflow on that commit to pass.

## Publish on GitHub

```bash
git tag -a v1.0.0 -m "RustingEngine v1.0.0"
git push origin v1.0.0
```

The release workflow builds Linux and Windows archives, creates the GitHub
Release, and attaches both archives. Generated game projects use the matching
Git tag when the editor is running without a local engine source checkout.
Do not move or recreate a published version tag; publish a patch version for
later fixes.

## After publishing

- Download both archives and confirm their editor executable starts.
- Create a clean project with the downloaded editor.
- Export its default scene and run the exported game on Windows and Linux.
- Record any driver-specific Vulkan problem in the GitHub Release notes.
