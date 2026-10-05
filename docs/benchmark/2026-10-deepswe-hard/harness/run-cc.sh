#!/bin/bash
# usage: run-cc.sh <job-name> <pier run args...>  — plain Claude Code baseline, same model/effort/version as AUTO
export PATH=/root/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin HOME=/root
cd /root/gb-bench
set -a; . ./bench.env; set +a
job=$1; shift
exec pier run -a claude-code -m 'deepseek-flash[1M]' --ak version=2.1.280 --ak reasoning_effort=max \
  -o /root/gb-bench/jobs --job-name "$job" -y "$@" > /root/gb-bench/$job.log 2>&1
