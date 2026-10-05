#!/bin/bash
# usage: run-job.sh <job-name> <pier run args...>
export PATH=/root/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin HOME=/root
cd /root/gb-bench
set -a; . ./bench.env; set +a
export PYTHONPATH=/root/gb-bench/adapter
job=$1; shift
exec pier run --agent-import-path gold_band_agent:GoldBandAuto -m 'deepseek-flash[1M]' \
  -o /root/gb-bench/jobs --job-name "$job" -y "$@" > /root/gb-bench/$job.log 2>&1
