#!/bin/bash
# usage: resume-job.sh <job-name> <error-type>...  — rerun only trials that failed with these exception types
export PATH=/root/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin HOME=/root
cd /root/gb-bench
set -a; . ./bench.env; set +a
export PYTHONPATH=/root/gb-bench/adapter
job=$1; shift
args=(); for e in "$@"; do args+=(-f "$e"); done
exec pier job resume -p /root/gb-bench/jobs/$job "${args[@]}" > /root/gb-bench/$job.resume.log 2>&1
