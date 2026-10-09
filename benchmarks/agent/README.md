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
- `hidden`: scenario files, relative to the task folder, run with
  `rusting test` after the agent finishes. The task passes when all pass.

## Running a task by hand

1. `rusting new <parent> Task --template <template>`, apply `seed` and
   `remove`.
2. Give the agent `request` in that project.
3. `rusting test <parent>/Task <task folder>/<hidden>` for each hidden
   scenario.

## Checks

`cargo test --test agent_benchmark` checks that every task parses, its
hidden scenarios parse, and its seed and reference edits apply to the
current template. `cargo test --test agent_benchmark -- --ignored` builds
each task twice: a hidden scenario must fail on the seeded project and every
hidden scenario must pass with the reference applied. Run it after changing
a template.
