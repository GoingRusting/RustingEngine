# RustingEngine Release Guide

## Before tagging

1. Update the version in `Cargo.toml` and add the same version to
   `CHANGELOG.md`.
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
