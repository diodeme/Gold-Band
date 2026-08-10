#!/usr/bin/env bash
#
# install-gold-band-macos.sh
# 一键安装 Gold Band（macOS / Apple Silicon）
# 流程：定位/下载 DMG -> 校验完整性 -> 移除隔离属性 -> 挂载并安装到 /Applications -> 移除 App 隔离属性
#
# 用法：
#   bash install-gold-band-macos.sh                 # 用 ~/Downloads 里的 DMG，找不到则自动下载
#   bash install-gold-band-macos.sh -y              # 同上，但已存在时自动覆盖（无需确认）
#   bash install-gold-band-macos.sh /path/to.dmg   # 指定本地 DMG
#   bash install-gold-band-macos.sh /path/to.dmg -y # 指定 DMG + 自动覆盖
#
# 环境变量（可选覆盖）：
#   GOLD_BAND_VERSION=0.10.0   GOLD_BAND_ARCH=aarch64   (Intel 机改 x64)
#
# 说明：
#   - Gold Band 的 Release 未提供 DMG 的官方校验和/签名文件（.dmg 无 .sig），
#     因此“校验”采用 GitHub API 返回的 asset 摘要（sha256）比对：
#     可防止下载截断/损坏，但【无法防范 GitHub Release 本身被篡改】——这是目前最佳可得手段。
#   - 安装到 /Applications 需要管理员权限（脚本会用 sudo 提示输入密码）。
#   - 应用未在 Apple 公证，安装后首次启动仍需：右键 /Applications/Gold Band.app -> 打开。

set -euo pipefail

VERSION="${GOLD_BAND_VERSION:-0.10.0}"
ARCH="${GOLD_BAND_ARCH:-aarch64}"            # Apple Silicon 用 aarch64；Intel 用 x64
DMG_NAME="Gold.Band_${VERSION}_${ARCH}.dmg"
REPO="diodeme/Gold-Band"
DL_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${DMG_NAME}"
APP_NAME="Gold Band.app"
INSTALL_PATH="/Applications/${APP_NAME}"

# 解析参数
DMG_PATH=""
FORCE_OVERWRITE=0
for a in "$@"; do
  case "$a" in
    -y|--yes) FORCE_OVERWRITE=1 ;;
    *)        [[ -z "$DMG_PATH" ]] && DMG_PATH="$a" ;;
  esac
done

echo "==> Gold Band 安装脚本 (macOS)"
echo "    版本: ${VERSION}  架构: ${ARCH}"
echo "    目标: ${INSTALL_PATH}"
echo

# ---------- 1. 定位或下载 DMG ----------
if [[ -z "$DMG_PATH" ]]; then
  DMG_PATH=$(ls -1 ~/Downloads/"${DMG_NAME}" 2>/dev/null | head -n1) || true
  if [[ -z "$DMG_PATH" ]]; then
    echo "==> 未在 ~/Downloads 找到 ${DMG_NAME}，准备从 GitHub 下载…"
    DMG_PATH="/tmp/${DMG_NAME}"
    curl -fL --retry 3 -o "$DMG_PATH" "$DL_URL"
    echo "    已下载到 $DMG_PATH"
  else
    echo "==> 使用本地文件: $DMG_PATH"
  fi
fi

if [[ ! -f "$DMG_PATH" ]]; then
  echo "错误: 找不到 DMG 文件: $DMG_PATH" >&2
  exit 1
fi

# ---------- 2. 校验完整性 ----------
echo "==> 校验 DMG 完整性…"

# 2a. 确认是有效的磁盘映像
if ! hdiutil imageinfo "$DMG_PATH" >/dev/null 2>&1; then
  echo "错误: 文件不是有效的 DMG 或已损坏: $DMG_PATH" >&2
  exit 1
fi

# 2b. 与 GitHub API 的 asset 摘要（sha256）比对
EXPECTED=""
API_JSON=$(curl -fsSL --retry 2 "https://api.github.com/repos/${REPO}/releases/tags/v${VERSION}" 2>/dev/null || true)
if [[ -n "$API_JSON" ]]; then
  EXPECTED=$(printf '%s' "$API_JSON" | /usr/bin/python3 - "$DMG_NAME" <<'PY' 2>/dev/null || true
import sys, json
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)
name = sys.argv[1]
for a in data.get("assets", []):
    if a.get("name") == name:
        print(a.get("digest", "").split(":", 1)[-1])
        break
PY
  )
fi

if [[ -n "$EXPECTED" ]]; then
  ACTUAL=$(shasum -a 256 "$DMG_PATH" | awk '{print $1}')
  if [[ "$EXPECTED" == "$ACTUAL" ]]; then
    echo "    ✅ SHA256 校验通过"
  else
    echo "错误: SHA256 不匹配！" >&2
    echo "    期望: $EXPECTED" >&2
    echo "    实际: $ACTUAL" >&2
    echo "    文件可能损坏或被篡改，已中止安装。" >&2
    exit 1
  fi
else
  echo "    ⚠️  未能获取官方 SHA256（无网络或 Release 未提供），仅做基础校验。"
  SIZE=$(stat -f%z "$DMG_PATH" 2>/dev/null || echo 0)
  if [[ "$SIZE" -lt 1000000 ]]; then
    echo "错误: 文件过小（${SIZE} 字节），可能下载不完整。" >&2
    exit 1
  fi
  echo "    文件大小: ${SIZE} 字节（基础检查通过）"
fi

# ---------- 3. 移除 DMG 的隔离属性 ----------
echo "==> 移除 DMG 隔离属性…"
xattr -dr com.apple.quarantine "$DMG_PATH" 2>/dev/null || \
  sudo xattr -dr com.apple.quarantine "$DMG_PATH"

# ---------- 4. 挂载并安装 ----------
echo "==> 挂载 DMG 并安装到 /Applications …"
MOUNT=$(mktemp -d -t goldband)
cleanup() { hdiutil detach "$MOUNT" >/dev/null 2>&1 || true; rmdir "$MOUNT" >/dev/null 2>&1 || true; }
trap cleanup EXIT

hdiutil attach "$DMG_PATH" -nobrowse -mountpoint "$MOUNT" >/dev/null

SRC_APP=$(find "$MOUNT" -maxdepth 2 -name '*.app' -type d 2>/dev/null | head -n1) || true
if [[ -z "$SRC_APP" ]]; then
  echo "错误: 在 DMG 中未找到 .app" >&2
  exit 1
fi

# 若已存在，确认覆盖
if [[ -d "$INSTALL_PATH" && "$FORCE_OVERWRITE" -ne 1 ]]; then
  read -r -p "    /Applications 已存在 ${APP_NAME}，是否覆盖？(y/N) " ans
  if [[ "$ans" != "y" && "$ans" != "Y" ]]; then
    echo "    已取消安装。"
    exit 0
  fi
fi

# 优先普通权限安装（admin 用户 /Applications 通常可写），sudo 作后备
sudo ditto -rsrc "$SRC_APP" "$INSTALL_PATH" 2>/dev/null || ditto -rsrc "$SRC_APP" "$INSTALL_PATH"
chown -R "$(id -un):admin" "$INSTALL_PATH" 2>/dev/null || sudo chown -R "$(id -un):admin" "$INSTALL_PATH" 2>/dev/null || true

# ---------- 5. 移除 App 的隔离属性 ----------
echo "==> 移除 App 隔离属性…"
xattr -dr com.apple.quarantine "$INSTALL_PATH" 2>/dev/null || \
  sudo xattr -dr com.apple.quarantine "$INSTALL_PATH"

# ---------- 完成 ----------
echo
echo "✅ 安装完成: $INSTALL_PATH"
echo
echo "⚠️  重要：由于应用未经过 Apple 公证，首次启动请手动操作一次："
echo "    在 Finder 中 右键 /Applications/Gold Band.app -> 打开，"
echo "    在弹窗中点「打开」即可将其加入白名单，之后双击即可正常启动。"
