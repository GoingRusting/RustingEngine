# Agent benchmark tasks

Each folder here is one task for a coding agent: a one-paragraph request,
the project the agent starts from, and hidden acceptance scenarios the agent
never sees. Milestone L9 in `roadmap.md` builds the runner and baselines on
top of this suite.

## Task format

`task.json` in each folder:

- `kind`: `new-game`, `feature`, `bug-fix` or `performance`.
- `template`: the `rusting new --template` the starting project comes from.
- `request`: the paragraph given to the agent, word for word.
- `seed`: edits that make the starting project, for example the bug a
  `bug-fix` task plants. Each is `{"file", "find", "replace"}`, and `find`
  must occur exactly once in the file.
- `remove`: project files deleted from the starting project, such as the
  template's own test when it is the hidden scenario.
- `reference`: edits, applied after `seed`, that make one correct solution.
- `reference_patches` (optional): scene patch files, relative to the task
  folder, that the reference applies to `scenes/main.rscene` with
  `rusting scene patch` after its edits. A `new-game` task needs them.
- `hidden`: scenario files, relative to the task folder, run with
  `rusting test` after the agent finishes. The task passes when all pass.

## Running a task by hand

1. `rusting new <parent> Task --template <template>`, apply `seed` and
   `remove`.
2. Give the agent `request` in that project.
3. `rusting test <parent>/Task <task folder>/<hidden>` for each hidden
   scenario.

## Running an agent

`run.py --agent '<command>' [task folder ...]` runs every task (or the named
ones) against any agent CLI. The command runs in the seeded project with the
request on standard input and in `RUSTING_BENCH_REQUEST`, and the `rusting`
under test first on `PATH`. The JSON report gives, per task, the hidden
scenario results, wall time, agent output size, and the agent's `rusting`
commands, failed commands, builds and corrective builds (a build right after
a failed one), taken from `RUSTING_COMMAND_LOG`. Turns, failed edits and
human interventions only the agent knows: a wrapper may write
`{"turns": n, "failed_edits": n, "interventions": n}` to
`RUSTING_BENCH_AGENT_REPORT`, and missing fields are null.

## Checks

`cargo test --test agent_benchmark` checks that every task parses, its
hidden scenarios parse, and its seed and reference edits apply to the
current template. `cargo test --test agent_benchmark -- --ignored` builds
each task twice: a hidden scenario must fail on the seeded project and every
hidden scenario must pass with the reference applied. Run it after changing
a template. Set `RUSTING_BENCH_TASK=<folder>` to check one task while
writing it.
