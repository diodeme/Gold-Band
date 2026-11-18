# 首次启动引导与 Antigravity 接入 P0 验证记录（历史）

> 历史范围说明（2026-10-20）：当前需求已移除 Antigravity 接入及 Gemini 退役迁移，收敛为首次启动引导。下文保留 2026-10-19 原方案的核验事实与当时门槛；其中 Antigravity 的阻塞及后续接入任务不再适用于当前计划。Node/npm 初核当时未包含现有 Gemini 条目，当前环境兼容性验证需按完整 Catalog 补齐。

> 日期：2026-10-19（Asia/Taipei）。分支：`codex/first-launch-antigravity`，基于 `main` 的 `bbef7724`。\
> 当时状态：原方案 P0 部分核验完成，官方 ACP 启动被当前 Windows 环境拒绝，原方案 P0 未通过。未修改产品实现，P1–P4 尚未开始。\
> 当前范围：[首次启动引导开发计划](首次启动引导开发计划.md)；[首次启动引导技术方案](../../产品设计文档/interaction/app/first-run-onboarding.md)。

## 1. 工作区与证据边界

从 2026-10-17 保存的 `stash@{0}` 恢复开发计划、产品技术方案，以及产品设计索引和 Agent 管理页中的引用；原 stash 保留。未恢复其中无关的根目录 Cargo profile 修改，工作区已有资源快照及桌面构建文件的修改保持原样。没有提交或推送。

本次核验目录：`target/onboarding-p0-20261019/`。保存 Registry 条目、包清单、npm engines 检查和启动失败记录。原始记录只描述本次观察，不代表各支持平台、真实登录或产品运行验收。

## 2. 官方分发核验

2026-10-19 读取 ACP 官方 Registry，`antigravity-acp` 当时的版本为 **1.4.1**。当前工作区快照中为 **1.4.0**；本次下载并检查的是该确定版本的 Windows x64 包，没有把新 Registry 版本自动写入产品快照。后续若改用 1.4.1，必须重新进行同一套运行验证。

来源：`https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`。条目保存于 `registry-antigravity.json`。

Registry 声明的分发平台及入口如下；“声明”不等于本次在该平台运行通过。

| 平台 | 入口 | 参数 |
| --- | --- | --- |
| Windows x86_64、aarch64 | `./agy_acp_server.exe` | 无 |
| macOS x86_64、aarch64 | `./agy_acp_server.par` | 无 |
| Linux x86_64、aarch64 | `./agy_acp_server.par` | `--uid=` |

本次下载地址：`https://dl.google.com/agy-extensions/releases/windows/agy-acp-server-1.4.0-windows-x86_64.zip`。

- ZIP 大小：125,646,049 bytes。
- ZIP SHA256：`D737AAD2D6388E071BC907DB415C129A256915D275F436E68F10DCDF6AF896DA`。
- 完整 ZIP 可读取并解压，包含以下两个文件；二者均为 Windows x64 PE，Authenticode 状态均为 `Valid`，签名主体均为 Google LLC。

| 文件 | 大小（bytes） | SHA256 |
| --- | ---: | --- |
| `agy_acp_server.exe` | 82,245,832 | `2AA1897144A60AAA1B8B54FC0595511AC6E6CE0CF2A7B3166CB1D0E4D4005AB8` |
| `localharness_external.exe` | 146,036,888 | `A5170FF005636B5E3FEDBCB0F61FA3204A2789C19EF38DFC424EC884A98E80F9` |

清单见 `package-manifest.json`。两个文件均保留原名与相邻位置进行启动尝试；辅助程序如何被查找、是否要求特定 cwd 尚未经过运行验证，不能只凭压缩包结构推导完整启动契约。

官方接入帮助 `https://antigravity.google/docs/ide/extensions` 本次请求连接失败，未获得可用于认证、安装目录或 Skills 目录配置的正文。

## 3. 启动阻塞与验收边界

环境：Microsoft Windows 11 专业版，`10.0.22621`，64 位。Python `subprocess.Popen` 直接启动完整路径的 `agy_acp_server.exe`，使用 `CREATE_NO_WINDOW`、管道标准输入输出、隔离工作目录及隔离 HOME/USERPROFILE/AppData；未传入用户凭据，未发送推理请求。

失败发生在 Windows `CreateProcess`，返回：

```text
PermissionError: [WinError 5] 拒绝访问。
```

没有取得进程句柄，没有执行到 `initialize`，不能将此次失败解释为 ACP 协议不兼容、需要登录或模型不可用。

只读检查确认：文件存在且签名有效，ACL 中有读取及执行权限，没有 `Zone.Identifier` 附加数据流。在检查的近期 CodeIntegrity、AppLocker 和 Defender 日志中没有找到能归因到该程序的阻止记录。因此**拒绝执行的具体原因仍未确定**，不能声称已经证明由某款安全软件、策略或供应商程序缺陷造成。

2026-10-19 用户确认没有其他可正常运行的官方 ACP Server 路径或可用测试环境。当前尚不能关闭 P0 必要门槛：

- `initialize`、`session/new` 与模型目录。
- 官方认证方法、未登录状态及外部认证完成后的重试。
- 模型切换、配置选项与权限请求。
- 取消、会话释放、辅助进程与进程树清理。
- ACP Server 与 CLI 的登录目录、Skills 目录是否共享。
- Windows ARM64、macOS、Linux 的实际运行。

后续需在系统允许执行官方程序的环境中继续原 P0 验证。不通过移动程序绕过策略、修改 ACL、关闭防护或改用社区协议桥消除当前阻塞；也不把 mock 测试当作官方运行证据。

## 4. Node/npm 候选兼容性

本机只读版本命令返回 Node.js `22.22.3`、npm `10.9.8`、npx `10.9.8`。这仅确认本次 shell 可运行它们，尚未验证 EXE 的 PATH 刷新、多安装选择或中文／空格路径。

从当前精选 Catalog 中筛出实际使用 npx 的条目（排除计划退役的 Gemini），读取 npm 官方对应版本元数据；使用 npm 生态的 `semver` 比较，不运行 `npx -y`、不安装 Agent、不修改全局 npm 配置。原始结果见 `compatibility.json`。

| 条目 | 固定版本 | 顶层 `engines.node` |
| --- | --- | --- |
| `@agentclientprotocol/claude-agent-acp` | `0.87.0` | `>=22` |
| `@agentclientprotocol/codex-acp` | `2.1.1` | 未声明 |
| `@tencent-ai/codebuddy-code` | `2.168.2` | 未声明 |
| `@qwen-code/qwen-code` | `0.25.2` | `>=22.0.0` |
| `pi-acp` | `0.0.34` | `>=20` |
| `npm` | `10.9.0` | `^18.17.0 || >=20.5.0` |
| `npm` | `11.19.1` | `^20.17.0 || >=22.9.0` |

对声明了范围的上述条目，Node.js `22.20.0`、`22.22.3`、`24.0.0`、`24.22.0` 均满足顶层 engines。未声明的项保持 unknown，不推定兼容。这些数据不证明传递依赖、原生程序、各操作系统或完整运行组合可用；设计中的最低版本仍为候选策略，尚未变更为发布承诺。

元数据来源为 `https://registry.npmjs.org/<package>/<version>`，精确请求地址及获取时间记录在原始结果中。

## 5. 代码核对与下一阶段边界

当前源码与方案的主要判断一致：

- `DesktopContext::from_workspace` 调用的 `load_configs` 会补语言并写 settings。首次身份判断必须早于这一步，不能在写入后再检查文件是否存在。
- Catalog 生成器只处理 npx/uvx；二进制分发默认得到空 command。接入 Antigravity 需要显式平台启动描述与校验，单纯替换 ID 不成立。
- 内置实例的 command/args 写盘时被剔除，读盘及运行配置加载时从 Catalog 注入；本机程序定位应保持独立字段，不能引入第二命令事实源。
- `load_workflow_template_store` 会迁移、更新内置模板并写回。预览不能直接使用这条带维护副作用的读取路径。
- `save_settings` 和模板存储各自单文件写入，现有 `with_file_lock` 是进程内锁；最终应用仍需方案中限定到两份配置的提交恢复边界。
- 已有 Doctor 的每 Agent 互斥、全局并发限制及配置 revision 校验可以复用，不能再建一套无界扫描调度。

P0 真实协议门槛通过后，继续按主计划 P1–P4 实现并补接口测试、UI 验证与生产性能证据。本轮没有运行功能单元测试、前端构建或 EXE 产品验收，因为没有产品实现变更，且官方进程尚未启动。

## 6. 影响复核与清理

过度设计：本轮仅恢复方案并补充验证记录，没有新增产品依赖、状态、缓存、队列或协议适配器。性能：没有产品运行路径变化，外部核验只读固定数量的公开元数据，不访问用户历史或凭据。

本次没有启动浏览器或前端服务；启动失败未创建 ACP 进程。核验完成后删除本次下载包、解压目录及隔离用户／工作目录，保留有界证据与复测脚本。共享规则已覆盖“真实根因与环境失败分开”和“Catalog 不等于实例能力”，无需新增经验规则。
