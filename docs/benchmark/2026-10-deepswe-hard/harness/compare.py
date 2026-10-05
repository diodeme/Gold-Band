import json, sys, glob, os, re
from datetime import datetime
def load(job):
    out = {}
    for t in glob.glob(f"/root/gb-bench/jobs/{job}/*/result.json"):
        r = json.load(open(t)); d = os.path.dirname(t)
        rw = (r.get("verifier_result") or {}).get("rewards") or {}
        try: mins = (datetime.fromisoformat(r["agent_execution"]["finished_at"]) - datetime.fromisoformat(r["agent_execution"]["started_at"])).total_seconds()/60
        except Exception: mins = (datetime.fromisoformat(r["finished_at"]) - datetime.fromisoformat(r["started_at"])).total_seconds()/60
        tok = {"in":0,"cached":0,"out":0}
        cc = os.path.join(d, "agent/claude-code.txt")
        if os.path.exists(cc):
            for line in open(cc, errors="ignore"):
                if '"type":"result"' in line:
                    try:
                        u = json.loads(line).get("usage") or {}
                        tok = {"in": u.get("input_tokens",0), "cached": u.get("cache_read_input_tokens",0), "out": u.get("output_tokens",0)}
                    except ValueError: pass
        out[os.path.basename(d).rsplit("__",1)[0]] = dict(reward=rw.get("reward"), f2p=f"{rw.get('f2p_passed')}/{rw.get('f2p_total')}", p2p=f"{rw.get('p2p_passed')}/{rw.get('p2p_total')}", mins=mins, tok=tok)
    return out
A_JOB, C_JOB = sys.argv[1], sys.argv[2]
a, c = load(A_JOB), load(C_JOB)
sys.path.insert(0, "/root/gb-bench"); import summarize
for t in sorted(a):
    g_ = glob.glob(f"/root/gb-bench/jobs/{A_JOB}/{t}__*/agent/gold-band-home.tgz")
    if not g_: a[t]["tok"]={"in":0,"cached":0,"out":0}; a[t]["nodes"]=0; continue
    d = g_[0]
    u = summarize.usage_from_home(__import__("pathlib").Path(d)); a[t]["tok"] = {"in":u["inputTokens"],"cached":u["cachedReadTokens"],"out":u["outputTokens"]}; a[t]["nodes"]=len(u["nodes"])
for t in sorted(a):
    x, y = a[t], c.get(t, {})
    print(f"{t:<34} AUTO {x['reward']} f2p={x['f2p']:<8} p2p={x['p2p']:<6} {x['mins']:5.1f}m n={x['nodes']} out={x['tok']['out']/1e3:.0f}k cached={x['tok']['cached']/1e6:.0f}M | CC {y.get('reward')} f2p={y.get('f2p'):<8} p2p={y.get('p2p'):<6} {y.get('mins',0):5.1f}m out={y.get('tok',{}).get('out',0)/1e3:.0f}k cached={y.get('tok',{}).get('cached',0)/1e6:.0f}M")
for k,arm in (("AUTO",a),("CC",c)):
    print(k, "solved", sum(1 for v in arm.values() if v["reward"]==1), "mins", round(sum(v["mins"] for v in arm.values())/len(arm),1), "in", sum(v["tok"]["in"] for v in arm.values()), "cached", sum(v["tok"]["cached"] for v in arm.values()), "out", sum(v["tok"]["out"] for v in arm.values()))
