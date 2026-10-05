#!/bin/bash
# Pull task images one at a time (ECR anonymous pulls rate-limit under concurrency).
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
for t in $(ls /root/gb-bench/subsets/hard-b23); do
  img=$(grep '^docker_image' /root/deep-swe/tasks/$t/task.toml | cut -d'"' -f2)
  for i in 1 2 3 4 5; do docker pull -q "$img" >/dev/null 2>&1 && { echo "ok $t"; break; }; echo "retry $i $t"; sleep $((i*20)); done
done
echo done
