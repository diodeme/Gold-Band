#!/usr/bin/env bash
# Install the latest Gold Band release on macOS without requiring Python or jq.

set -euo pipefail

readonly REPO="diodeme/Gold-Band"
readonly APP_NAME="Gold Band.app"
readonly EXPECTED_BUNDLE_ID="local.gold-band.desktop"
readonly INSTALL_DIR="${GOLD_BAND_INSTALL_DIR:-/Applications}"
readonly INSTALL_PATH="${INSTALL_DIR}/${APP_NAME}"

VERSION="${GOLD_BAND_VERSION:-}"
ARCH="${GOLD_BAND_ARCH:-}"
DMG_PATH=""
FORCE_OVERWRITE=0

TEMP_DIR=""
MOUNT_PATH=""
STAGE_DIR=""
STAGE_APP=""
BACKUP_DIR=""
BACKUP_APP=""
INSTALL_COMMITTED=0

usage() {
  cat <<'EOF'
Usage: install-gold-band-macos.sh [--yes] [path-to-dmg]

Options:
  -y, --yes  Replace an existing installation without prompting.
  -h, --help Show this help text.

Environment:
  GOLD_BAND_VERSION      Release version, for example 0.12.4. Defaults to latest.
  GOLD_BAND_ARCH         aarch64 or x64. Defaults to the current Mac architecture.
  GOLD_BAND_INSTALL_DIR  Installation directory. Defaults to /Applications.
EOF
}

die() {
  printf '错误: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "缺少 macOS 系统命令: $1"
}

run_install_command() {
  if [[ -w "$INSTALL_DIR" ]]; then
    "$@"
  else
    sudo "$@"
  fi
}

remove_install_temp_dir() {
  local path="$1"
  [[ -n "$path" ]] || return 0
  case "$path" in
    "$INSTALL_DIR"/.gold-band-install.*|"$INSTALL_DIR"/.gold-band-backup.*)
      run_install_command rm -rf -- "$path" 2>/dev/null || true
      ;;
  esac
}

cleanup() {
  local exit_code=$?
  set +e

  if [[ -n "$MOUNT_PATH" ]]; then
    hdiutil detach "$MOUNT_PATH" >/dev/null 2>&1 || true
  fi

  if [[ "$INSTALL_COMMITTED" -ne 1 && -n "$BACKUP_APP" && -d "$BACKUP_APP" && ! -e "$INSTALL_PATH" ]]; then
    run_install_command mv "$BACKUP_APP" "$INSTALL_PATH" >/dev/null 2>&1 || true
  fi

  remove_install_temp_dir "$STAGE_DIR"
  if [[ "$INSTALL_COMMITTED" -eq 1 || -z "$BACKUP_APP" || ! -d "$BACKUP_APP" ]]; then
    remove_install_temp_dir "$BACKUP_DIR"
  elif [[ -n "$BACKUP_DIR" ]]; then
    printf '警告: 自动恢复旧 App 失败，备份保留在 %s\n' "$BACKUP_DIR" >&2
  fi

  if [[ -n "$TEMP_DIR" ]]; then
    rm -rf -- "$TEMP_DIR"
  fi

  exit "$exit_code"
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes)
      FORCE_OVERWRITE=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      if [[ $# -gt 1 ]]; then
        die "只能指定一个 DMG 路径"
      fi
      if [[ $# -eq 1 ]]; then
        DMG_PATH="$1"
      fi
      break
      ;;
    -*)
      die "未知参数: $1"
      ;;
    *)
      [[ -z "$DMG_PATH" ]] || die "只能指定一个 DMG 路径"
      DMG_PATH="$1"
      ;;
  esac
  shift
done

for command_name in curl hdiutil plutil shasum codesign ditto xattr find mktemp mkdir mv rm rmdir uname tr; do
  require_command "$command_name"
done

[[ "$(uname -s)" == "Darwin" ]] || die "该脚本只能在 macOS 上运行"
[[ -d "$INSTALL_DIR" ]] || die "安装目录不存在: $INSTALL_DIR"
if [[ ! -w "$INSTALL_DIR" ]]; then
  require_command sudo
fi

if [[ -z "$ARCH" ]]; then
  case "$(uname -m)" in
    arm64|aarch64) ARCH="aarch64" ;;
    x86_64|amd64) ARCH="x64" ;;
    *) die "无法识别当前 Mac 架构；请设置 GOLD_BAND_ARCH=aarch64 或 x64" ;;
  esac
fi
case "$ARCH" in
  aarch64|x64) ;;
  *) die "不支持的架构: $ARCH（仅支持 aarch64 或 x64）" ;;
esac

if [[ -n "$VERSION" && ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
  die "无效的版本号: $VERSION"
fi

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/gold-band-install.XXXXXX")"
if [[ -z "$VERSION" ]]; then
  readonly RELEASE_JSON="${TEMP_DIR}/release.json"
  printf '==> 获取 Gold Band 最新 Release…\n'
  curl --fail --silent --show-error --location --retry 2 \
    --output "$RELEASE_JSON" "https://api.github.com/repos/${REPO}/releases/latest" || \
    die "无法读取 GitHub 最新 Release；请检查网络或显式设置 GOLD_BAND_VERSION"

  RELEASE_TAG="$(plutil -extract tag_name raw -o - "$RELEASE_JSON" 2>/dev/null || true)"
  [[ "$RELEASE_TAG" == v* ]] || die "GitHub Release 缺少有效 tag_name"
  VERSION="${RELEASE_TAG#v}"
  [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || \
    die "GitHub Release 返回无效版本号: $VERSION"
fi

readonly DMG_NAME="Gold.Band_${VERSION}_${ARCH}.dmg"
readonly DOWNLOAD_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${DMG_NAME}"
readonly CHECKSUM_URL="${DOWNLOAD_URL}.sha256"

CHECKSUM_PATH="${TEMP_DIR}/${DMG_NAME}.sha256"
printf '==> 下载 Release SHA256…\n'
curl --fail --silent --show-error --location --retry 2 \
  --output "$CHECKSUM_PATH" "$CHECKSUM_URL" || \
  die "Release 未提供 ${DMG_NAME}.sha256；该安装器只支持包含 checksum 的新版本"

read -r EXPECTED_SHA256 CHECKSUM_NAME < "$CHECKSUM_PATH" || \
  die "无法读取 ${DMG_NAME}.sha256"
EXPECTED_SHA256="$(printf '%s' "$EXPECTED_SHA256" | tr '[:upper:]' '[:lower:]')"
[[ "$EXPECTED_SHA256" =~ ^[0-9a-f]{64}$ ]] || die "Release SHA256 格式无效"
[[ "$CHECKSUM_NAME" == "$DMG_NAME" ]] || \
  die "Release SHA256 文件名不匹配（期望 ${DMG_NAME}，实际 ${CHECKSUM_NAME:-missing}）"

if [[ -z "$DMG_PATH" ]]; then
  DOWNLOADS_DMG="${HOME}/Downloads/${DMG_NAME}"
  if [[ -f "$DOWNLOADS_DMG" ]]; then
    DMG_PATH="$DOWNLOADS_DMG"
    printf '==> 使用本地文件: %s\n' "$DMG_PATH"
  else
    DMG_PATH="${TEMP_DIR}/${DMG_NAME}"
    printf '==> 下载 Gold Band %s (%s)…\n' "$VERSION" "$ARCH"
    curl --fail --location --retry 3 --output "$DMG_PATH" "$DOWNLOAD_URL" || \
      die "下载 DMG 失败: $DOWNLOAD_URL"
  fi
fi

[[ -f "$DMG_PATH" ]] || die "找不到 DMG 文件: $DMG_PATH"

printf '==> 校验 DMG SHA256 与磁盘映像完整性…\n'
read -r ACTUAL_SHA256 _ < <(shasum -a 256 "$DMG_PATH")
ACTUAL_SHA256="$(printf '%s' "$ACTUAL_SHA256" | tr '[:upper:]' '[:lower:]')"
[[ "$ACTUAL_SHA256" == "$EXPECTED_SHA256" ]] || \
  die "SHA256 不匹配（期望 ${EXPECTED_SHA256}，实际 ${ACTUAL_SHA256}）"
hdiutil verify "$DMG_PATH" >/dev/null || die "DMG 内部完整性校验失败"

MOUNT_PATH="${TEMP_DIR}/mount"
mkdir "$MOUNT_PATH"
hdiutil attach "$DMG_PATH" -readonly -nobrowse -mountpoint "$MOUNT_PATH" >/dev/null

SRC_APP="$(find "$MOUNT_PATH" -maxdepth 2 -type d -name "$APP_NAME" -print -quit)"
[[ -n "$SRC_APP" ]] || die "DMG 中未找到 ${APP_NAME}"

INFO_PLIST="${SRC_APP}/Contents/Info.plist"
[[ -f "$INFO_PLIST" ]] || die "App 缺少 Contents/Info.plist"
BUNDLE_ID="$(plutil -extract CFBundleIdentifier raw -o - "$INFO_PLIST" 2>/dev/null || true)"
[[ "$BUNDLE_ID" == "$EXPECTED_BUNDLE_ID" ]] || \
  die "App 标识不匹配（期望 ${EXPECTED_BUNDLE_ID}，实际 ${BUNDLE_ID:-missing}）"
codesign --verify --deep --strict "$SRC_APP" || die "App 代码签名完整性校验失败"

if [[ -e "$INSTALL_PATH" && "$FORCE_OVERWRITE" -ne 1 ]]; then
  read -r -p "    ${INSTALL_PATH} 已存在，是否替换？(y/N) " answer
  if [[ "$answer" != "y" && "$answer" != "Y" ]]; then
    printf '    已取消安装。\n'
    exit 0
  fi
fi

printf '==> 安装到 %s…\n' "$INSTALL_PATH"
STAGE_DIR="$(run_install_command mktemp -d "${INSTALL_DIR}/.gold-band-install.XXXXXX")"
STAGE_APP="${STAGE_DIR}/${APP_NAME}"
run_install_command ditto -rsrc "$SRC_APP" "$STAGE_APP"
run_install_command xattr -dr com.apple.quarantine "$STAGE_APP"

STAGED_BUNDLE_ID="$(plutil -extract CFBundleIdentifier raw -o - "${STAGE_APP}/Contents/Info.plist" 2>/dev/null || true)"
[[ "$STAGED_BUNDLE_ID" == "$EXPECTED_BUNDLE_ID" ]] || die "暂存 App 标识校验失败"
codesign --verify --deep --strict "$STAGE_APP" || die "暂存 App 代码签名完整性校验失败"

if [[ -e "$INSTALL_PATH" ]]; then
  BACKUP_DIR="$(run_install_command mktemp -d "${INSTALL_DIR}/.gold-band-backup.XXXXXX")"
  BACKUP_APP="${BACKUP_DIR}/${APP_NAME}"
  run_install_command mv "$INSTALL_PATH" "$BACKUP_APP"
fi

if ! run_install_command mv "$STAGE_APP" "$INSTALL_PATH"; then
  if [[ -n "$BACKUP_APP" && -d "$BACKUP_APP" ]]; then
    run_install_command mv "$BACKUP_APP" "$INSTALL_PATH" || true
  fi
  die "无法把暂存 App 切换为正式安装"
fi

if ! codesign --verify --deep --strict "$INSTALL_PATH"; then
  run_install_command mv "$INSTALL_PATH" "$STAGE_APP" || true
  if [[ -n "$BACKUP_APP" && -d "$BACKUP_APP" ]]; then
    run_install_command mv "$BACKUP_APP" "$INSTALL_PATH" || true
  fi
  die "安装后的 App 代码签名完整性校验失败，已恢复旧版本"
fi

INSTALL_COMMITTED=1
remove_install_temp_dir "$BACKUP_DIR"
BACKUP_DIR=""
BACKUP_APP=""

printf '\n✅ 安装完成: %s\n' "$INSTALL_PATH"
printf '   已在完整性与 App 身份校验后移除 quarantine，可直接从 Finder 打开。\n'
