# macOS 安装与排错指南

Gold Band 的 macOS 发行包（`.dmg`）目前**未经过 Apple 公证（Notarization）**，也没有使用 Apple Developer ID 进行代码签名。因此 macOS 的 Gatekeeper 会在首次打开时拦截应用。本文说明原因、解决方法，并提供一个一键安装脚本。

> 仓库中的 `TAURI_SIGNING_PRIVATE_KEY` / `.sig` 仅用于 Tauri 自更新器（对应 `plugins.updater.pubkey`），**与 Gatekeeper 无关**。

## 1. 为什么会被拦截

macOS 的 Gatekeeper 在首次打开从网络下载的应用时会检查：

1. 是否用 **Apple Developer ID 证书**做了代码签名；
2. 是否通过 **Apple 公证**（提交 Apple 服务器扫描并拿到回执）。

本项目的 DMG 这两步都没有做（构建 `identifier` 为 `local.gold-band.desktop`，属本地/开发构建），因此首次打开会出现以下提示之一：

- “无法验证‘Gold Band’是否包含可能危害 Mac 的恶意软件”
- “‘Gold Band’已损坏，无法打开。请推出磁盘映像。”

> 这是**安全机制拦截，不是安装包损坏**。文件本身正常，提示里的“已损坏”是 Gatekeeper 对“无法验证”应用的统一措辞。

## 2. 选择正确的架构

- **Apple Silicon（M 系列，如 M5）**：请下载 `aarch64` 版本（如 `Gold.Band_x.y.z_aarch64.dmg`）。这是 M 系列的原生架构。
- **Intel / 老机器**：使用 `x64` 版本。注意 Apple 已逐步废弃 Rosetta 转译，`x64` 包可能在新版 macOS 中被标记为“Intel 芯片的软件后续不再支持”，优先选 `aarch64`。

## 3. 手动安装步骤（推荐）

1. 挂载 DMG，将 `Gold Band.app` **拖到 `/Applications`** 完成安装。
2. **推出（卸载）** 该 DMG。
3. 移除隔离属性（只删 `xattr`，不改 App 内容，解决 Gatekeeper 拦截）：

   ```bash
   xattr -dr com.apple.quarantine /Applications/Gold\ Band.app
   ```

4. **右键（Control+单击）`Gold Band.app` → 打开**，在弹窗中点「打开」一次，将其加入 Gatekeeper 白名单。之后双击即可正常启动。

> ⚠️ 不要用 `sudo spctl --master-disable` 关闭全局 Gatekeeper——那会放行所有未签名软件，是过度操作。只放行这一个 App 即可，系统 SIP/沙盒/防火墙都照常生效。

## 4. 一键安装脚本

`scripts/install-gold-band-macos.sh` 自动完成：定位/下载 DMG → 校验完整性 → 移除隔离属性 → 挂载并用 `ditto` 安装到 `/Applications` → 移除 App 隔离属性。

```bash
bash scripts/install-gold-band-macos.sh            # 用 ~/Downloads 里的 DMG，找不到则自动下载
bash scripts/install-gold-band-macos.sh -y         # 已存在时自动覆盖（跳过确认）
bash scripts/install-gold-band-macos.sh <path>     # 指定本地 DMG 路径
```

脚本最后会提示你仍需**手动右键「打开」一次**（应用未公证，绕不开的一次性 GUI 确认）。

**校验边界（重要）**：该 Release 暂未为 DMG 提供官方校验和/签名文件（`.sig` 仅覆盖 deb/rpm/AppImage/exe/msi/app.tar.gz，唯独 DMG 没有）。因此脚本采用 GitHub API 返回的 asset `sha256` 比对本地文件，能防**下载截断/损坏**，但**无法防范 GitHub Release 本身被篡改**。请始终从官方仓库 `github.com/diodeme/Gold-Band` 的 Release 下载。无网络时脚本降级为本地大小检查并明确告警。

## 5. 已知问题：打开后立即退出（已在 0.12.4 修复）

在 v0.12.0 ~ v0.12.3 区间，如果本机**曾运行过更新版本的 dev 构建**（会把全局设置 `~/.gold-band/settings.json` 的 `settingsSchemaVersion` 写成 `3`），再用**旧版 DMG**（二进制只支持到 schema v2）打开时，应用会**启动即退出，且无崩溃报告**。直接启动二进制的报错为：

```
failed to start Gold Band desktop: settings schema version 3 is newer than supported version 2
```

原因：应用内部有设置 schema 版本守卫（`src/config/mod.rs`），读到的版本比二进制支持的新就主动退出（干净退出，故无崩溃报告）。

解决方法（按风险从低到高）：

- **升级到 v0.12.4 或更高**：客户端内置更新已支持 schema v3，可正常打开（推荐）。
- 或手动把 `~/.gold-band/settings.json` 的 `settingsSchemaVersion` 从 `3` 改成 `2`（改前务必备份该文件）。

> 教训：避免在同一台机器上混用“旧版发布 DMG”和“新版 dev 构建”——二者会互相改写设置 schema 版本，导致打不开。想用稳定 DMG 时，就别再跑 dev 版写设置。

## 6. 推荐路径：客户端更新

从 v0.12.4 起，应用内置的 Tauri 更新器（使用 `plugins.updater.pubkey` 自签名校验）可正常工作，直接在客户端内升级即可，无需手动处理 Gatekeeper。
