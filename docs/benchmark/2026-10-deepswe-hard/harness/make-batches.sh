set -euo pipefail
cd /root/gb-bench
install -m 644 /mnt/g/gb-bench/adapter/gold_band_agent.py adapter/gold_band_agent.py
install -m 644 /mnt/g/gb-bench/hard-subset.txt hard-subset.txt
grep -v '^#' hard-subset.txt | awk '{print $1}' > /tmp/hard.txt
wc -l < /tmp/hard.txt
for t in $(cat /tmp/hard.txt); do [ -d /root/deep-swe/tasks/$t ] || echo "MISSING $t"; done
# fixed-seed shuffle so each batch is a representative mix
python3 -c "
import random; t=open('/tmp/hard.txt').read().split(); random.Random(20261004).shuffle(t)
open('/root/gb-bench/hard-order.txt','w').write('\n'.join(t)+'\n')"
rm -rf subsets; n=0
for t in $(cat hard-order.txt); do n=$((n+1)); b=$(( n<=10 ? 1 : (n<=20 ? 2 : 3) ))
  mkdir -p subsets/hard-b$b; ln -s /root/deep-swe/tasks/$t subsets/hard-b$b/$t; done
for b in 1 2 3; do echo "batch $b: $(ls subsets/hard-b$b | tr '\n' ' ')"; done
cd adapter && /root/.local/share/uv/tools/datacurve-pier/bin/python -c "import gold_band_agent as g; a=g.GoldBandAuto.__new__(g.GoldBandAuto); import json; print(json.dumps({k:v for k,v in g.GoldBandAuto._auto_config(a).items()}))"
