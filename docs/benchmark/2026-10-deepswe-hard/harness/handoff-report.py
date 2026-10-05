"""Handoff-file smoke report: completion size, path usage, validation errors, repairs.

usage: python3 handoff-report.py <job-name>
"""
import collections, glob, json, statistics, sys, tarfile

job = sys.argv[1]
sizes, inline, path_fields = [], 0, collections.Counter()
codes, repairs_exhausted = collections.Counter(), 0
for trial in sorted(glob.glob(f"/root/gb-bench/jobs/{job}/*/")):
    tgz = trial + "agent/gold-band-home.tgz"
    name = trial.rstrip("/").split("/")[-1]
    try:
        reward = json.load(open(trial + "verifier/reward.json"))
    except (OSError, ValueError):
        reward = "?"
    try:
        tar = tarfile.open(tgz)
    except OSError:
        print(f"{name}: no gold-band-home.tgz yet (reward={reward})")
        continue
    nodes = 0
    with tar:
        for m in tar:
            if m.name.endswith("artifacts/dynamic-node-completion.json"):
                s = tar.extractfile(m).read().decode("utf-8", "ignore")
                nodes += 1
                sizes.append(len(s))
                try:
                    o = json.loads(s)
                except ValueError:
                    codes["unparseable-artifact"] += 1
                    continue
                if "summary" in o:
                    inline += 1
                path_fields["summaryPath"] += "summaryPath" in o
                nxt = o.get("next") or {}
                for spec in [nxt.get("node"), nxt.get("merge"), nxt.get("acceptance"), *(nxt.get("nodes") or [])]:
                    if isinstance(spec, dict):
                        path_fields["taskPath"] += "taskPath" in spec
                        inline += "task" in spec
            elif m.name.endswith("/dynamic/graph.json"):
                graph = json.load(tar.extractfile(m))
                for proposal in graph.get("proposals", []):
                    for error in proposal.get("validationErrors", []):
                        codes[error.get("code")] += 1
            elif m.name.endswith("/dynamic/events.jsonl"):
                for line in tar.extractfile(m):
                    event = json.loads(line)
                    code = (((event.get("data") or {}).get("runtimeError") or {}).get("code") or {}).get("code")
                    if code == "dynamic.completion.repair-exhausted":
                        repairs_exhausted += 1
    print(f"{name}: reward={reward} completion-artifacts={nodes}")

if sizes:
    print(f"completion JSON chars: n={len(sizes)} median={statistics.median(sizes):.0f} max={max(sizes)}")
print(f"path fields: {dict(path_fields)}  inline summary/task fields: {inline}")
print(f"rejected-proposal error codes: {dict(codes) or 'none'}")
print(f"repair-exhausted pauses: {repairs_exhausted}")
