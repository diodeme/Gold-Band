set -euxo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq ca-certificates curl git build-essential pkg-config libssl-dev unzip jq >/dev/null
if ! command -v docker >/dev/null; then curl -fsSL https://get.docker.com | sh >/dev/null; fi
systemctl enable --now docker
if ! command -v node >/dev/null; then curl -fsSL https://deb.nodesource.com/setup_22.x | bash - >/dev/null; apt-get install -y -qq nodejs >/dev/null; fi
if ! command -v uv >/dev/null && [ ! -x /root/.local/bin/uv ]; then curl -LsSf https://astral.sh/uv/install.sh | sh >/dev/null; fi
export PATH=/root/.local/bin:$PATH
uv tool install -q datacurve-pier || uv tool upgrade datacurve-pier
cd /root && [ -d deep-swe ] || git clone -q https://github.com/datacurve-ai/deep-swe
docker --version; node --version; npm --version; git --version; uv --version; pier --version || pier --help | head -5
docker run --rm hello-world | head -2
