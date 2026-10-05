"""Live view of a Pier job running Gold Band AUTO.

usage: python3 watch.py <job-name> [--loop SECONDS]

Per trial: phase, elapsed minutes, AUTO node graph progress (done/running/total,
fanout groups, workspaces), token usage so far, and the graded reward once done.
"""

import json
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

JOBS = Path("/root/gb-bench/jobs")
DYNAMIC_GLOB = "/installed-agent/home/.gold-band/projects/*/tasks/task-001/runs/run-001/rounds/round-001/nodes/ai-dynamic/attempt-001/dynamic"
PROBE = (
    f"d=$(ls -d {DYNAMIC_GLOB} 2>/dev/null | head -1); [ -z \"$d\" ] && exit 0; "
    "cat $d/graph.json; echo; echo '@@USAGE@@'; "
    "find $d -name acp.prompt-usage.jsonl -exec cat {} + 2>/dev/null"
)


def sh(args: list[str]) -> str:
    try:
        return subprocess.run(args, capture_output=True, text=True, timeout=30).stdout
    except subprocess.TimeoutExpired:
        return ""


def running_containers() -> dict[str, str]:
    names = sh(["docker", "ps", "--format", "{{.Names}}"]).split()
    return {n.removesuffix("-main-1"): n for n in names if n.endswith("-main-1")}


def container_started(name: str) -> datetime | None:
    raw = sh(["docker", "inspect", "-f", "{{.State.StartedAt}}", name]).strip()
    try:
        return datetime.fromisoformat(raw.replace("Z", "+00:00")[:26] + "+00:00")
    except ValueError:
        return None


def probe(container: str) -> tuple[dict | None, dict]:
    out = sh(["docker", "exec", container, "sh", "-c", PROBE])
    graph_text, _, usage_text = out.partition("@@USAGE@@")
    graph = None
    if graph_text.strip():
        try:
            graph = json.loads(graph_text)
        except ValueError:
            graph = None
    usage = {"in": 0, "cached": 0, "out": 0}
    for line in usage_text.splitlines():
        try:
            record = json.loads(line)
        except ValueError:
            continue
        if record.get("kind") == "promptCompleted":
            u = record.get("usage") or {}
            usage["in"] += u.get("inputTokens") or 0
            usage["cached"] += u.get("cachedReadTokens") or 0
            usage["out"] += u.get("outputTokens") or 0
    return graph, usage


def graph_summary(graph: dict | None) -> str:
    if not graph:
        return "starting"
    nodes = graph.get("nodes") or []
    nodes = list(nodes.values()) if isinstance(nodes, dict) else nodes
    done = sum(1 for n in nodes if n.get("status") == "completed")
    running = [n.get("id", "?") for n in nodes if n.get("status") == "running"]
    groups = graph.get("groups") or []
    workspaces = graph.get("workspaces") or []
    nested = sum(1 for g in groups if isinstance(g, dict) and g.get("parentGroupId"))
    fanout = f" fanout={len(groups)}" + (f"(nested {nested})" if nested else "") if groups else ""
    ws = f" ws={len(workspaces)}" if len(workspaces) > 1 else ""
    return f"nodes {done}/{len(nodes)}{fanout}{ws} run:{','.join(running) or '-'}"


def fmt_tokens(usage: dict) -> str:
    if not any(usage.values()):
        return ""
    return f"in={usage['in'] / 1e3:.0f}k cached={usage['cached'] / 1e6:.1f}M out={usage['out'] / 1e3:.0f}k"


def done_line(trial: Path, result: dict) -> str:
    rewards = (result.get("verifier_result") or {}).get("rewards") or {}
    exc = (result.get("exception_info") or {}).get("exception_type")
    code = (trial / "agent" / "exit-code")
    exit_code = code.read_text().strip() if code.is_file() else "-"
    if exc:
        return f"ERROR {exc} (exit={exit_code})"
    reward = rewards.get("reward")
    mark = "PASS" if reward == 1 else "FAIL"
    return f"{mark} f2p={rewards.get('f2p_passed', '-')}/{rewards.get('f2p_total', '-')} exit={exit_code}"


def render(job: str) -> None:
    job_dir = JOBS / job
    trials = sorted(p for p in job_dir.iterdir() if p.is_dir()) if job_dir.is_dir() else []
    containers = running_containers()
    now = datetime.now(timezone.utc)
    counts = {"PASS": 0, "FAIL": 0, "ERROR": 0, "running": 0, "pending": 0}
    print(f"== {job}  {datetime.now():%H:%M:%S}")
    for trial in trials:
        task = trial.name.rsplit("__", 1)[0]
        result_file = trial / "result.json"
        result = json.loads(result_file.read_text()) if result_file.is_file() else {}
        if result.get("finished_at"):
            line = done_line(trial, result)
            counts[line.split()[0]] += 1
            print(f"  {task:<46} {line}")
            continue
        container = containers.get(trial.name.lower())
        if not container:
            counts["pending"] += 1
            print(f"  {task:<46} pending (image pull / build / verify)")
            continue
        counts["running"] += 1
        started = container_started(container)
        mins = f"{(now - started).total_seconds() / 60:5.1f}m" if started else "   ?"
        graph, usage = probe(container)
        print(f"  {task:<46} {mins} {graph_summary(graph)} {fmt_tokens(usage)}")
    load = Path("/proc/loadavg").read_text().split()[0]
    mem = sh(["free", "-g"]).splitlines()[1].split()
    print(
        f"  -- pass={counts['PASS']} fail={counts['FAIL']} error={counts['ERROR']} "
        f"running={counts['running']} pending={counts['pending']} | load={load} mem={mem[2]}G/{mem[1]}G"
    )


def main() -> None:
    job = sys.argv[1]
    interval = int(sys.argv[sys.argv.index("--loop") + 1]) if "--loop" in sys.argv else 0
    while True:
        if interval:
            print("\033[2J\033[H", end="")
        render(job)
        if not interval:
            break
        time.sleep(interval)


if __name__ == "__main__":
    main()
