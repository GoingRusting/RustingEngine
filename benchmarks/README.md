# Render benchmark baselines

The fixed render benchmark is the vertical-slice performance gate. The scene
and camera path live in `src/runtime/render_benchmark.rs`, and the runner and
baseline comparison live in `src/rendering/benchmark.rs`.

The scene contains:

- a fixed ground;
- 40 pillars, each topped by a sphere, using five PBR materials;
- 1,000 GPU physics bodies from `PhysicsBenchmark::Mixed`, falling onto the
  ground and raising landing events;
- a shadowed sun and 24 point lights, which is more than Eco's light budget.

The camera orbits once in 600 frames and dives in twice, from 40 m to 16 m.
Physics advances one 60 Hz tick per frame.

## Running

```sh
cargo run --release --example render_bench -- eco benchmarks/<machine>-eco.json
```

The first argument is the quality profile: `auto`, `eco`, `balanced` or
`high`. The run renders 600 frames at 1920x1080 into an offscreen image and
prints the report as JSON.

If the baseline file does not exist, the run writes it. If the file exists,
the run compares against it, prints every regression, and exits with
status 1 when there is any. The comparison rules are:

- Work counters are compared when the extent, quality and frame count match.
  These are draws, dispatches, triangles, visible instances, GPU memory,
  upload bytes, and event and state readback bytes. More than 10% growth is
  a regression.
- Times (frame, CPU and GPU, at the 95th percentile) are compared only on the
  same device and driver. Growth is a regression when it is more than 10% and
  more than 0.5 ms.
- Any overflow that the baseline did not have is a regression. This covers
  dropped lights, dropped physics events, rejected physics commands, and
  higher readback latency.

The runner waits for each frame to finish before it starts the next one.
The frame time therefore includes CPU and GPU time with no overlap. It is an
upper bound on a presented frame and does not include presentation.

## Tested configurations

All runs were at 1920x1080 for 600 frames, with a release build, on CachyOS
Linux (kernel 7.2.8). Times are 95th percentiles in milliseconds.

| Device | Driver | Quality | Frame | CPU | GPU | Dropped lights |
|---|---|---|---|---|---|---|
| NVIDIA GeForce RTX 3060 | 615.71.09 | Eco | 3.0 | 0.24 | 2.2 | 9 |
| NVIDIA GeForce RTX 3060 | 615.71.09 | Balanced | 4.0 | 0.24 | 3.0 | 0 |
| NVIDIA GeForce RTX 3060 | 615.71.09 | High | 4.1 | 0.24 | 3.1 | 0 |
| NVIDIA GeForce RTX 3060 | 615.71.09 | Auto (High) | 4.1 | 0.25 | 3.1 | 0 |
| llvmpipe (LLVM 22.1.8) | Mesa 26.2.3 | Eco | 62.6 | 0.34 | 4.6* | 9 |

\* llvmpipe runs the GPU work on the CPU. Its timestamps do not capture that
work, so use the frame time instead.

Every run recorded the same work: 21 draws, 104,856 triangles, and 1,079
visible instances at most per frame.

`rtx3060-linux-eco.json` is the stored baseline for the RTX 3060.

**Not yet tested:** the Milestone 7 target of 1080p at 60 FPS on Intel UHD
620-class integrated graphics. Also untested are Windows, AMD GPUs, and
presented (windowed) frame pacing. To add a machine, run the command above
with a new baseline path, and add a row to this table.
