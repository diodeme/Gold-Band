#!/bin/bash
# Writes /root/gb-bench/bench.env (mode 600) from the local Claude Code settings backup.
# The token is read at run time and never stored in this repository.
set -euo pipefail
node -e '
const s=JSON.parse(require("fs").readFileSync("/mnt/c/Users/diode/.claude/settings.json.bak","utf8")).env;
const out=["ANTHROPIC_BASE_URL="+s.ANTHROPIC_BASE_URL,"ANTHROPIC_AUTH_TOKEN="+s.ANTHROPIC_AUTH_TOKEN,"GOLD_BAND_BINARY=/root/gb-bench/gold-band"].join("\n")+"\n";
require("fs").writeFileSync("/root/gb-bench/bench.env",out,{mode:0o600});'
