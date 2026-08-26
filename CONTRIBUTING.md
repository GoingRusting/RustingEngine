# Contributing to RustingEngine

Thank you for helping improve RustingEngine. Bug fixes, performance work,
documentation, tests, editor improvements, and focused new features are welcome.

## Before starting

- Check open GitHub issues and `roadmap.md` to avoid duplicating active work.
- For a large feature or architecture change, open an issue first and describe
  the problem, proposed public API, and expected performance or maintenance cost.
- Keep pull requests focused. Unrelated cleanup makes correctness and performance
  changes harder to review.

## Development setup

Install stable Rust, `rustfmt`, `clippy`, and a working Vulkan driver. Linux and
Windows are the primary supported platforms.

```bash
git clone https://github.com/GoingRusting/RustingEngine.git
cd RustingEngine
cargo build --workspace
./scripts/run_editor.sh
```

Run the complete local release gate before submitting a pull request:

```bash
./scripts/check_release.sh
```

The same formatting, strict Clippy, test, documentation, and release-build
checks run in GitHub Actions.

## Code guidelines

- Format Rust code with `cargo fmt` and keep Clippy clean with warnings denied.
- Prefer clear names and small modules. Keep source files below roughly 2,000
  lines by moving independent responsibilities into their own module.
- Add simple comments that explain *why* a field, system, synchronization step,
  or unusual calculation exists. Avoid difficult wording and comments that only
  repeat the code.
- Preserve the dependency direction described in `architecture.md`: gameplay
  ECS state is canonical, while renderer batches and GPU buffers are derived.
- Avoid a per-object allocation, lock, draw call, or GPU readback in high-volume
  paths. Make expensive work proportional to changes whenever possible.
- Add tests for bug fixes and public behavior. GPU layout changes also need size
  and offset coverage or shader-reflection validation.
- Do not commit `target`, `dist`, cooked `.rscene.bin` files, local editor
  settings, or machine-specific absolute paths.

## Performance pull requests

Never report only an FPS number. Include:

- the exact scene and object count;
- CPU, GPU, RAM, operating system, and driver when known;
- resolution, VSync/FPS-limit state, and Debug or Release build;
- before and after frame time, preferably with several runs;
- whether the bottleneck is CPU, GPU, synchronization, memory, or presentation.

Do not reduce correctness, remove synchronization, silently drop physics bodies,
or weaken a shader merely to increase the counter. Mention any visual or
simulation tradeoff explicitly.

## Pull request checklist

- Explain the user-visible problem and the chosen solution.
- Link the related issue when one exists.
- Add or update tests and documentation where behavior changed.
- Keep existing scene files and migrations compatible, or document the required
  format-version change.
- Confirm `./scripts/check_release.sh` passes.
- Include screenshots for editor changes and measurements for performance work.

By submitting a contribution, you confirm that you have the right to provide it
under the repository's [Rusting Engine License 1.0](LICENSE.md).
