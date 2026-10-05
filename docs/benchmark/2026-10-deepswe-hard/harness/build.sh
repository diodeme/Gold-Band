set -euo pipefail
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
docker run --rm --entrypoint sh public.ecr.aws/d3j8x8q7/swe-bench-202605:kh75679ajj3b8dtd7se3h7z0a1833y6r-v1.1 -c 'node --version; npm --version'
mkdir -p /root/gb-src /root/cargo-cache
apt-get install -y -qq rsync >/dev/null
rsync -a --delete --exclude target --exclude node_modules --exclude 'web/dist' --exclude '.git' --exclude 'src-tauri/target' --exclude tmp /mnt/e/Projects/Code/AI/Gold-Band/ /root/gb-src/
du -sh /root/gb-src
docker run --rm -v /root/gb-src:/src -v /root/cargo-cache:/usr/local/cargo/registry -v /root/gb-target:/target -e CARGO_TARGET_DIR=/target -w /src rust:1-bookworm \
  cargo build --release --locked -p gold-band --bin gold-band 2>&1 | tail -5
ls -la /root/gb-target/release/gold-band && ldd /root/gb-target/release/gold-band
