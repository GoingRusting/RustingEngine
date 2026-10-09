#!/usr/bin/env python3
"""Runs agent benchmark tasks against any agent CLI and writes a JSON report.

    benchmarks/agent/run.py --agent 'my-agent --yes' [task folder ...]

For each task the runner makes the seeded project, runs the agent command
in it with the request on standard input (and in RUSTING_BENCH_REQUEST),
then runs the hidden scenarios. Every `rusting` command the agent runs is
recorded through RUSTING_COMMAND_LOG. An agent wrapper may write
`{"turns": n, "failed_edits": n, "interventions": n}` to the file named by
RUSTING_BENCH_AGENT_REPORT; fields it leaves out are reported as null.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SUITE = Path(__file__).resolve().parent
ENGINE = SUITE.parent.parent
BUILDS = {"build", "check", "test", "run", "determinism", "export", "cook"}


def apply(root, edits):
    for edit in edits:
        path = root / edit["file"]
        text = path.read_text()
        if text.count(edit["find"]) != 1:
            sys.exit(f"`{edit['find']}` must occur once in {edit['file']}")
        path.write_text(text.replace(edit["find"], edit["replace"], 1))


def rusting(binary, *args, env=None):
    done = subprocess.run(
        [binary, *args, "--json"], capture_output=True, text=True, env=env
    )
    return json.loads(done.stdout)


def command_stats(log):
    """Counts from the agent's `rusting` command log."""
    lines = log.read_text().splitlines() if log.exists() else []
    commands = [json.loads(line) for line in lines]
    builds = [c for c in commands if c["args"] and c["args"][0] in BUILDS]
    # A build right after a failed build is a corrective one.
    corrective = sum(
        1 for before, after in zip(builds, builds[1:]) if not before["ok"]
    )
    return {
        "commands": len(commands),
        "failed_commands": sum(1 for c in commands if not c["ok"]),
        "builds": len(builds),
        "corrective_builds": corrective,
    }


def run_task(folder, agent, binary, timeout):
    task = json.loads((folder / "task.json").read_text())
    parent = Path(tempfile.mkdtemp(prefix="rusting-agent-run-"))
    created = rusting(binary, "new", str(parent), "Task", "--template", task["template"])
    if not created["ok"]:
        sys.exit(f"{folder.name}: {created}")
    root = parent / "Task"
    apply(root, task["seed"])
    for file in task["remove"]:
        (root / file).unlink()

    log = parent / "commands.jsonl"
    agent_report = parent / "agent.json"
    env = dict(os.environ)
    env["PATH"] = f"{Path(binary).parent}{os.pathsep}{env['PATH']}"
    env["RUSTING_COMMAND_LOG"] = str(log)
    env["RUSTING_BENCH_REQUEST"] = task["request"]
    env["RUSTING_BENCH_AGENT_REPORT"] = str(agent_report)
    started = time.monotonic()
    try:
        done = subprocess.run(
            agent, shell=True, cwd=root, env=env, input=task["request"],
            capture_output=True, text=True, timeout=timeout,
        )
        exit_code, output = done.returncode, done.stdout + done.stderr
        timed_out = False
    except subprocess.TimeoutExpired as expired:
        exit_code, timed_out = None, True
        output = (expired.stdout or b"").decode(errors="replace")
    wall = time.monotonic() - started

    hidden = {}
    for scenario in task["hidden"]:
        result = rusting(binary, "test", str(root), str(folder / scenario))
        hidden[scenario] = result["ok"]
    reported = json.loads(agent_report.read_text()) if agent_report.exists() else {}
    report = {
        "task": folder.name,
        "kind": task["kind"],
        "passed": all(hidden.values()),
        "hidden": hidden,
        "wall_s": round(wall, 1),
        "agent_exit_code": exit_code,
        "timed_out": timed_out,
        "output_bytes": len(output.encode()),
        **command_stats(log),
        "turns": reported.get("turns"),
        "failed_edits": reported.get("failed_edits"),
        "interventions": reported.get("interventions"),
    }
    shutil.rmtree(parent)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--agent", required=True, help="shell command that runs the agent")
    parser.add_argument("--rusting", default=str(ENGINE / "target/debug/rusting"))
    parser.add_argument("--timeout", type=float, default=3600, help="seconds per task")
    parser.add_argument("--out", help="report file (default: standard output)")
    parser.add_argument("tasks", nargs="*", help="task folders (default: all)")
    args = parser.parse_args()
    folders = [Path(t).resolve() for t in args.tasks] or sorted(
        p for p in SUITE.iterdir() if (p / "task.json").is_file()
    )
    results = [run_task(f, args.agent, args.rusting, args.timeout) for f in folders]
    report = {
        "engine": rusting(args.rusting, "version")["data"]["engine_version"],
        "agent": args.agent,
        "pass_rate": sum(r["passed"] for r in results) / len(results),
        "tasks": results,
    }
    text = json.dumps(report, indent=2)
    if args.out:
        Path(args.out).write_text(text + "\n")
    else:
        print(text)


if __name__ == "__main__":
    main()
