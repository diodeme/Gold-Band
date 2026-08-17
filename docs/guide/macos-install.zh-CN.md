# macOS 安装与排错指南

<!-- DOCS-I18N:START -->

[English](./macos-install.md) | **中文**

<!-- DOCS-I18N:END -->

Gold Band 的 macOS Release 在尚未配置 Apple Developer Program 凭证时，由 Tauri bundler 完成 ad-hoc 签名，但没有 Developer ID 签名和 Apple 公证。因此，从网络下载后首次打开可能被 Gatekeeper 拦截。本页提供 macOS 原生手动流程，以及只面向带 `.sha256` 校验文件的新 Release 的安装脚本。

> 发布流水线使用的 Tauri updater private key / `.sig` 只服务应用内更新完整性校验，与 Gatekeeper 的 Developer ID 和 Apple 公证不是同一套信任体系。

## 1. 选择正确的架构

- Apple Silicon（M 系列）：下载 `aarch64` 版本，例如 `Gold.Band_x.y.z_aarch64.dmg`。
- Intel Mac：下载 `x64` 版本。

安装脚本默认根据 `uname -m` 选择架构，也可以通过 `GOLD_BAND_ARCH=aarch64` 或 `GOLD_BAND_ARCH=x64` 显式指定。

## 2. 手动安装

1. 从项目的官方 GitHub Release 下载对应架构的 DMG。
2. 挂载 DMG，把 `Gold Band.app` 拖到 `/Applications`，然后推出 DMG。
3. 在 Finder 中按住 Control 点击 `Gold Band.app`，选择“打开”，再确认一次“打开”。

这是 macOS 为单个未公证 App 提供的例外流程。不要使用 `sudo spctl --master-disable` 全局关闭 Gatekeeper。

## 3. 安装脚本

`scripts/install-gold-band-macos.sh` 不依赖 Python、jq、Homebrew 或 Xcode Command Line Tools，只使用 macOS 自带的 `curl`、`plutil`、`shasum`、`hdiutil`、`codesign`、`ditto` 和 `xattr`。

```bash
bash scripts/install-gold-band-macos.sh
bash scripts/install-gold-band-macos.sh --yes
GOLD_BAND_VERSION=0.13.0 bash scripts/install-gold-band-macos.sh
GOLD_BAND_VERSION=0.13.0 bash scripts/install-gold-band-macos.sh ./Gold.Band_0.13.0_aarch64.dmg
```

脚本按固定顺序执行：

1. 未指定版本时，通过 GitHub Release API 的 `tag_name` 获取 latest 版本。
2. 下载或复用 `~/Downloads` 中名称完全匹配的 DMG。
3. 下载同一 Release 中的 `${DMG_NAME}.sha256`，校验摘要中的文件名和 SHA256。
4. 使用 `hdiutil verify` 校验 DMG 内部完整性。
5. 只接受名称为 `Gold Band.app`、bundle identifier 为 `local.gold-band.desktop` 且通过 `codesign --verify --deep --strict` 的 App。
6. 在 `/Applications` 同一文件系统内暂存并再次校验，替换旧 App；切换失败时恢复旧版本。
7. 所有校验通过后，只对暂存 App 移除 `com.apple.quarantine`，随后可直接从 Finder 打开。

脚本只支持 checksum 发布流程接入后、同时包含 `.sha256` sidecar 的新 Release。历史 Release 不包含该资产，如需安装请使用上面的手动流程。

## 4. 校验边界

两条 Release workflow 会在所有平台资产上传完成后，以流式 SHA256 为 DMG、App updater archive、EXE、MSI、AppImage、DEB 和 RPM 生成同名 `.sha256`。安装脚本缺少 sidecar、摘要格式错误、文件名不匹配或实际摘要不一致时都会终止，不提供弱校验 fallback。

DMG 与 `.sha256` 来自同一个 GitHub Release，因此该机制可以发现下载损坏、截断和资产错配，但不能抵御官方 GitHub 仓库或 Release 发布权限本身被攻破。拿到 Apple Developer Program 凭证后，仍应由现有 Tauri release 流程完成 Developer ID 签名和公证。
