"""Summarize a Pier job of Gold Band AUTO trials: reward, runtime and token usage.

usage: python3 summarize.py <job-dir>
Token totals come from Gold Band's per-turn `acp.prompt-usage.jsonl`
(`promptCompleted.usage`), summed over every dynamic node of the run.
"""

import io
import json
import sys
import tarfile
from datetime import datetime
from pathlib import Path


def usage_from_home(tgz: Path) -> dict:
    totals = {"inputTokens": 0, "outputTokens": 0, "cachedReadTokens": 0, "turns": 0, "nodes": set()}
    if not tgz.is_file():
        return totals
    with tarfile.open(tgz) as tar:
        for member in tar:
            if not member.name.endswith("acp.prompt-usage.jsonl"):
                continue
            node = member.name.split("/nodes/")[-1].split("/")[0]
            for line in io.TextIOWrapper(tar.extractfile(member), encoding="utf-8"):
                record = json.loads(line)
                if record.get("kind") != "promptCompleted":
                    continue
                usage = record.get("usage") or {}
                for key in ("inputTokens", "outputTokens", "cachedReadTokens"):
                    totals[key] += usage.get(key) or 0
                totals["turns"] += 1
                totals["nodes"].add(node)
    return totals


def minutes(result: dict) -> float | None:
    try:
        start = datetime.fromisoformat(result["started_at"])
        end = datetime.fromisoformat(result["finished_at"])
    except (KeyError, TypeError, ValueError):
        return None
    return (end - start).total_seconds() / 60


def main() -> None:
    job = Path(sys.argv[1])
    rows = []
    for trial in sorted(p for p in job.iterdir() if p.is_dir()):
        result_file = trial / "result.json"
        if not result_file.is_file():
            continue
        result = json.loads(result_file.read_text())
        rewards = (result.get("verifier_result") or {}).get("rewards") or {}
        agent = trial / "agent"
        exit_code = (agent / "exit-code").read_text().strip() if (agent / "exit-code").is_file() else "-"
        usage = usage_from_home(agent / "gold-band-home.tgz")
        rows.append(
            {
                "task": trial.name.rsplit("__", 1)[0],
                "reward": rewards.get("reward"),
                "f2p": f"{rewards.get('f2p_passed', '-')}/{rewards.get('f2p_total', '-')}",
                "exit": exit_code,
                "exception": (trial / "exception.txt").is_file(),
                "minutes": minutes(result),
                "nodes": len(usage["nodes"]),
                "turns": usage["turns"],
                "input": usage["inputTokens"],
                "cached": usage["cachedReadTokens"],
                "output": usage["outputTokens"],
            }
        )

    for row in rows:
        mins = f"{row['minutes']:.1f}" if row["minutes"] is not None else "-"
        print(
            f"{row['task']:<36} reward={row['reward']} f2p={row['f2p']:<7} exit={row['exit']} "
            f"exc={int(row['exception'])} min={mins:<6} nodes={row['nodes']} turns={row['turns']} "
            f"in={row['input']} cached={row['cached']} out={row['output']}"
        )
    graded = [r for r in rows if r["reward"] is not None]
    if rows:
        solved = sum(1 for r in graded if r["reward"] == 1)
        print(
            f"\ntrials={len(rows)} graded={len(graded)} solved={solved} "
            f"pass_rate={solved / len(rows):.3f} "
            f"input={sum(r['input'] for r in rows)} cached={sum(r['cached'] for r in rows)} "
            f"output={sum(r['output'] for r in rows)}"
        )


if __name__ == "__main__":
    main()
