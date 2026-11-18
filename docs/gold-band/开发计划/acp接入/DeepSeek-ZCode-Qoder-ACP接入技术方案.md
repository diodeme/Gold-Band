# DeepSeek Harness / ZCode / Qoder ACP 接入技术方案

> 状态：评审修订稿，未实施。调研与修订时间：2026-11-09。
> 实施准入：DeepSeek Harness、Qoder 在本文验收门槛通过后进入第一阶段；ZCode 仅确认架构可行，真实 ZCode 后端验证通过前不得进入发布 Catalog。

## 1. 结论与范围

本需求的根因不是 Gold Band 缺少新的 provider client，而是现有方案尚未完整区分以下事实：

1. Catalog 只提供构建期候选模板与来源信息，不代表本机可运行或具备某项会话能力。
2. Agent 的 ACP 能力必须来自当前真实 `initialize`、Doctor 和会话结果，不能来自旧 README、Agent ID 或品牌推断。
3. 平台支持、凭据持久化、生成产物原子性和恢复语义属于通用接入契约，不能在三个 Agent 的调用点分别打补丁。
4. “代码可桥接”不等于“达到发布准入”；没有真实后端、认证、工具调用和恢复证据的条目不得进入精选 Catalog。

| Agent | 稳定 ID | 集成来源 | 候选启动模板 | 修订后结论 |
| --- | --- | --- | --- | --- |
| DeepSeek Harness | `deepseek-harness` | DeepSeek 官方 CLI / ACP profile | `dsh --profile acp` | 第一阶段候选；必须补真实工具调用、停止、恢复和 Skill 验收 |
| Qoder | `qoder` | 官方 ACP Registry | `npx -y @qoder-ai/qodercli@1.1.86 --acp`，本机优先 `qodercli --acp` | 第一阶段候选；Registry fallback 与本机 CLI 两条路径必须分别验收 |
| ZCode（智谱） | `zcode-acp` | 社区桥接器 `zcode-acp-server` | `zcode-acp-server` | 仅架构可行；真实 ZCode 后端握手与会话能力验收前阻断发布 |

当前发布 Catalog 为 11 项。实施分两阶段：

- 第一阶段完成通用契约、DeepSeek Harness 与 Qoder，目标为 13 项。
- 第二阶段仅在 ZCode 发布门槛全部通过后加入 ZCode，目标为 14 项。

不新增 provider-specific client、并行状态机或 Agent 实例 canonical state。继续复用现有 ACP client、Doctor、首次引导、Skill 同步、Agent 管理页和 `ManagedAgentConfig`；只补齐 Catalog 元数据、统一平台判定、生成事务和接口级验收。

## 2. 现有接入链路与修订边界

| 链路 | 当前实现 | 本方案要求 |
| --- | --- | --- |
| Catalog 生成 | `scripts/prepare-agent-catalog.mjs` 从官方 Registry 生成 snapshot、Catalog 与图标 | 增加受控本地补充源、逐项 provenance、类型化平台限制和事务化发布 |
| Rust 加载 | `resources/agent-catalog.json` 编译进 `src/agent_catalog.rs` | Catalog 仍是只读构建产物；统一校验 schema、来源、平台和目录字段 |
| Agent 实例 | `ManagedAgentConfig::from_catalog` 创建实例；内置 `command/args` 由当前 Catalog 注入 | 不新增实例字段；现有实例继续保留名称、图标、环境变量、目录和 executable location |
| 本机发现 | 优先本地 ACP 命令或已准备的 npx 包；只有 npx launcher 不算 Agent 已安装 | 先做统一平台过滤，再执行现有只读发现；不自动下载未准备 Agent |
| Doctor | 通用 ACP 初始化、空会话、能力目录与有界清理 | 不把 `available_commands_update` 当成功条件；补充真实 prompt、工具与恢复验收 |
| 会话恢复 | 根据 `sessionCapabilities` 与 `loadSession` 决定 resume/load | 为每个新增 Agent 固定 new、stop、continue、load/resume、history、cleanup 契约 |
| Skill 同步 | 主目录可按全局/项目拆分，兼容目录只读 | 按 Agent 实际扫描路径配置；项目写入路径必须经后端接口验证 |

产品文档中的历史冲突必须同时收敛：内置 Agent 的 `command/args` 由当前客户端 Catalog 唯一维护，升级后既有实例也使用新 Catalog 启动配置；自定义 Agent 才由用户维护启动命令。不得继续保留“Catalog 只影响之后新建实例”的旧口径。

## 3. 数据结构与领域归属

### 3.1 Catalog provenance

Catalog 每个条目增加构建期来源信息：

```text
CatalogProvenance
  sourceKind: official-registry | local-supplement
  integrationOrigin: official | community-adapter
  validatedVersion: string
```

- `sourceKind` 说明条目定义来自官方 Registry 还是 Gold Band 本地精选补充源。
- `integrationOrigin` 说明 ACP 实现由供应商官方维护还是社区桥接，不通过英文 `description` 暗示。
- `validatedVersion` 表示 Gold Band 完整执行本文真实验收的版本，不表示自动锁定用户本机版本。
- 官方 Registry 条目的 `validatedVersion` 来自 `configs/agent-catalog-policy.json` 的显式 `validatedVersions` 映射；Registry 升级只改变候选 `version`，不得自动改写验收证据。本地补充条目在自身 provenance 中声明同一字段。
- `validatedVersions` 必须覆盖全部正式官方条目；本地补充 ID 不得重复放入 policy。生成器复用现有 `semver` 库按 Rust 加载契约校验非空精确版本，拒绝 `latest`、范围、`v` 前缀和空白包裹；预发布与 build metadata 合法时原样保留。构建结果与 staging 发布边界均校验完整 provenance；缺失验收证据在下载和发布前失败。
- 没有完整真实验收的版本不能写入 `validatedVersion`，也不能进入发布 Catalog。ZCode 在第二阶段准入前不生成正式 Catalog 条目。
- UI 只消费结构化 `integrationOrigin`，通过七语言 key 展示简短来源标签或帮助说明；上游 `description` 不承担信任边界或对客本地化。

这些字段属于构建期 Catalog，不复制到 `SettingsConfig.agents`，不形成第二份 Agent 实例事实源。

### 3.2 类型化平台约束

禁止使用 `win32-arm64` 这类把 Node、Rust 和产品命名拼接在一起的字符串。Catalog 使用稳定 wire format：

```text
PlatformSelector
  os: windows | macos | linux
  arch: x86_64 | aarch64

AgentCatalogEntry
  excludedPlatforms: PlatformSelector[]
```

运行时由一个平台规范化入口把 Rust `std::env::consts::{OS, ARCH}` 转成上述类型，并由一个 `supports_platform(entry, current_platform)` 判定函数统一消费。Web 不再自行解释 `win32`、`arm64` 或拼接字符串，只接收后端已过滤的 Catalog 投影。

平台限制的写入者与消费方：

1. 构建脚本只校验 `os/arch` 枚举、重复项和 schema，不承担运行时平台推断。
2. Rust Catalog 加载器校验并保存类型化约束。
3. onboarding 候选生成调用统一判定，排除当前平台不支持的条目。
4. Agent 管理的 Catalog 投影调用同一判定，不把不支持条目发送给新增选择器。
5. `create_agent` 后端再次调用同一判定，防止绕过 UI；返回结构化错误 `agent.unsupported-platform`，参数至少包含 `agentType`、`os`、`arch`。

已经配置的实例、workflow 引用和 Doctor 结果不得因 Catalog 新增限制而被删除或隐藏；用户仍可查看、编辑本机路径和诊断。平台限制只阻止从内置 Catalog 新建不受支持的实例。

### 3.3 能力与持久化边界

- Catalog 是候选模板，不声明当前实例必然支持 resume、load、image、MCP 或工具调用。
- Doctor capability snapshot 与正式 ACP 会话是运行能力事实源。
- `AcpAdapterConfig.env` 会随 Agent 配置持久化；它不是临时进程环境，也不是系统密钥库。
- Session、timeline、stop/continue 和 cleanup 继续使用现有会话 canonical state，不为三个 Agent 新增旁路生命周期。

## 4. 候选调研与修订后的接入契约

### 4.1 DeepSeek Harness

官方入口为 `dsh --profile acp`。`dsh` 是公开 launcher，ACP 是 profile，不是独立公共 bin。用户数据根目录默认 `~/.dsh`；Skill 文件系统提供方读取项目 `.dsh/skills`、项目 `.agents/skills`、用户 `~/.dsh/skills` 与用户 `~/.agents/skills`。

当前验证基线为 `@deepseek-ai/dsh@0.2.1-rc.2`，Node `v22.22.3`。已完成的最小握手证据：

- `initialize` 成功，`agentInfo.name = deepseek-harness-acp`。
- `promptCapabilities` 声明 image/audio/embedded context 为 false。
- `mcpCapabilities.http = true`。
- `sessionCapabilities` 声明 close/list/resume，未声明 `loadSession`。
- `session/new` 返回模型和 `reasoning_effort` 配置目录，携带 stdio MCP server 也可建立。

旧版 `@deepseek-ai/dsh-acp` 的 automation-only 说明不能作为 0.2.1-rc.2 的完整能力依据。当前适配器已经包含 committed thought、通用 tool lifecycle、config update、usage/context update 的事件映射；但握手或源码存在映射不等于 Gold Band 已完成真实显示验收。因此发布前必须执行至少一个真实工具调用回合，确认 thought、tool start/update/completion、usage/context 和最终正文均能进入现有通用事件模型，不能只测文本回复。

恢复策略严格按实测 capability：

- 普通继续在声明可用时使用 `session/resume`。
- 未声明 `loadSession` 时不得尝试完整外部历史加载；Gold Band 只展示自身已持久化 timeline。
- 图片、音频和 embedded resource 按现有路径文本策略降级；stdio MCP 继续随 `session/new` 传入，远程 MCP 只按实际 capability 开放。
- Catalog 不声明 system prompt metadata 能力，沿用现有首轮 user turn 内的隐藏稳定上下文；恢复同一会话时不重复注入。
- Doctor 清理按 capability 选择 delete/close；未声明 delete 时不得把 delete 失败当成 Agent 不可用，close 与进程树回收仍必须有界完成。
- UI 使用现有 composer 处理中状态，不因某一阶段暂时没有正文误判失败，也不伪造工具进度。

### 4.2 Qoder

Qoder 使用官方 ACP Registry 条目 `qoder@1.1.86`。Registry fallback 固定为：

```text
npx -y @qoder-ai/qodercli@1.1.86 --acp
```

本机安装路径单独使用：

```text
qodercli --acp
```

两条路径必须独立验收，不能用本机较新 `qoder` CLI 的成功结果代替 Registry 固定版本，也不能用 Registry fallback 代替本机发现路径。

2026-11-12 修订：本机路径的验收标准是 PATH 中实际解析到的 `qodercli` 命令；该命令可由官方 native 安装提供，也可由用户通过 npm 全局安装的同名命令提供。验收记录必须写明命令解析来源与实际版本。不得把“必须执行 native 二进制”作为 Windows x64 发布门槛；native 二进制被终端安全软件拦截时，作为环境事实记录并保留供应链证据。

认证主路径为供应商的 `qoder login`。`QODER_PERSONAL_ACCESS_TOKEN` 仅作为用户显式选择的高级环境变量方式；由于 `AcpAdapterConfig.env` 会持久化到本机 settings，产品和技术方案不得再声称 Gold Band “不保存 token”。Agent 编辑器在 env 区域说明其内容会保存到本机配置，优先建议使用供应商登录；日志、诊断摘要和遥测不得输出环境变量值。本文不新增密钥库或临时凭据状态。

当前 1.1.86 验证重点：

- `loadSession = true`，并提供 session list；必须真实验证 list/load/history，而不是只断言 initialize JSON。
- 验证模型目录、权限、图片输入声明、MCP、停止与 cleanup。
- npm 路径要求 Node >= 20；仍需进入统一版本/engines/平台矩阵。
- 官方限制 Windows ARM64，Catalog 使用 `excludedPlatforms: [{ os: windows, arch: aarch64 }]`。
- 首次引导只在发现本机 `qodercli` 或确认固定 1.1.86 npx 包已准备时启动 Doctor，不因存在 npx launcher 自动下载；用户在 Agent 管理页显式创建后仍可走现有受控 npx 准备流程。
- ACP 通用 client 必须读取 initialize 的 `authMethods`；`session/new` 返回结构化 AuthRequired 时，优先使用已配置的 `env_var` 方法认证并重试一次。终端交互认证和缺失环境变量返回 `provider.auth-required`，不无限重试。
- Qoder Catalog 的供应商差异通过构建期 `launchOverrides` 表达；不得在 ACP client 中增加 Qoder ID 分支。

### 4.3 ZCode（智谱）

候选桥接器为社区项目 `zcode-acp-server@0.82.3`，Node >= 22。它调用真实 `zcode app-server --stdio`，架构上可以复用 Gold Band 通用 ACP client，但社区项目声明不能替代真实兼容性证据。

2026-11-11 的旧证据只支持“架构可行，尚未达到发布准入”：当时没有安装真实 ZCode，固定 0.82.3 桥接器 120 秒内没有返回 `initialize`。2026-11-12 安装官方桌面端 3.17.2 与内置 CLI 0.17.1，并将桥接器升级到 0.84.1 后，真实 GLM 后端已通过 initialize、new、真实工具、stop、list、load/history、显式 resume、fork、进程清理与三层 Skill 验收（见《ZCode-验收记录-2026-11-12.md》）；0.84.1 在 Windows 上自动发现 CLI 与 sqlite-capable Node，不要求手工 `ZCODE_BIN`/`ZCODE_NODE`。close/delete 未声明，清理契约映射为“关闭 stdin 后进程自然退出”。其余平台矩阵未测试。ZCode 已按第二阶段准入加入正式 Catalog，来源保持 `local-supplement/community-adapter` 并固定 `0.84.1`。

第二阶段发现只认已经安装的 native `zcode-acp-server`；桥接器再通过 `ZCODE_BIN`、PATH 或自身支持的安装位置寻找 ZCode。Gold Band 不自动下载社区桥接器，也不为 ZCode 添加专属桌面目录扫描。

桥接器实际 Skill 扫描路径为：

- 用户：`~/.zcode/skills`、`~/.agents/skills`。
- 项目：`<cwd>/.agents/skills`。

没有证据支持项目 `<cwd>/.zcode/skills`。第二阶段若通过真实验收，Catalog 目录映射应为：

- `primaryAgentDir: .zcode`。
- `projectPrimaryAgentDir: .agents`。
- `compatibleAgentDirs: [.agents]`，扫描时按规范路径去重。

这使 Gold Band 全局写入 `~/.zcode/skills`，项目写入 `<project>/.agents/skills`，同时读取用户通用目录；不得向项目 `.zcode/skills` 写入。项目写入必须通过真实后端/Skill 接口测试确认，而不是只断言目录字符串。

现有目录数据结构能够表达该策略，不新增全局/项目两套 compatible 字段：`.agents` 在全局作用域是额外读取目录，在项目作用域与项目主目录相同，由 `skill_directory_policy` 按作用域去重。必须修正当前编辑输入把“与任一主目录相同”的 compatible 目录全局删除的归一化逻辑；只有当某目录同时等于全局主目录和项目有效主目录、因而在两个作用域都冗余时才删除。比较前统一规范化相对目录的分隔符、`.` 段和 Windows 大小写，保存时保留用户配置的相对目录语义。否则 ZCode 经 Agent 管理保存一次后会丢失用户 `~/.agents/skills` 的读取能力。

ZCode 第二阶段发布门槛：

1. 安装真实 ZCode 与桥接器，完成认证和 `initialize`。
2. 验证 new、真实 prompt、工具/权限、stop、continue、resume/load、history、fork 和 cleanup。
3. 验证用户与项目 Skill 实际被 ZCode 发现，且不写项目 `.zcode/skills`。
4. 验证缺失 ZCode、版本不兼容、认证失败和协议超时均返回结构化原因并回收进程树。
5. 核对桥接器 license、repository、发布包内容和固定版本供应链信息。

## 5. Catalog 生成与发布事务

### 5.1 本地精选补充源

新增 `resources/acp-registry-local.json`，只保存未进入官方 Registry、且已通过发布准入的 Gold Band 精选条目：

- 第一阶段只加入 `deepseek-harness`。
- `qoder` 继续来自官方 Registry。
- `zcode-acp` 只有在第二阶段真实验收通过后才加入本地补充源和 `BUILTIN_AGENT_IDS`。

合并规则：

1. 读取官方 Registry snapshot 或在线 Registry。
2. 读取 local supplement；官方与本地 ID 冲突时构建失败，不定义覆盖优先级。
3. 第一阶段 `BUILTIN_AGENT_IDS` 为 13 项；第二阶段通过后为 14 项。
4. online 模式只刷新官方 snapshot，不重写 local supplement。
5. offline 模式固定读取已提交 snapshot 与 local supplement，保证可重复构建。
6. Catalog 顶层 source 同时记录官方 Registry 与 local supplement 的位置、版本/摘要和生成时间；每个条目写入自己的 provenance。

### 5.2 事务化生成

snapshot、Catalog 和图标由单一发布函数管理同一事务；恢复判断以整批目标为单位，不能逐文件接受旧版或新版：

1. 在仓库外或系统临时目录创建本次 staging 目录，不直接写正式路径。
2. 在 staging 中生成 snapshot、Catalog 和完整图标集合；offline 模式复制并校验已有图标，不访问网络。
3. 对 staging 执行全部校验：schema、ID/顺序、provenance、固定版本、平台枚举、目录字段、HTTPS SVG、文件名、内容摘要和预期数量。
4. 所有校验通过后，为每个正式目标准备同目录临时文件与旧内容备份，再原子写入 schema 2 recovery journal。每条记录显式区分 `originalExists` / `newExists`，分别记录旧、新摘要、目标、临时文件和备份；事务阶段为 `publishing` 或 `rolling-back`。
5. 使用原子 rename/replace 发布；旧 Catalog 管理、但新版不再使用的图标也作为删除记录参与事务。提交前普通失败必须先记录 `rolling-back`，再恢复整个旧集合，即使异常发生在最后一个替换之后也必须回滚。恢复使用备份副本加原子替换，验证完整旧集合前不消耗备份，保证恢复过程可以重复执行。
6. 进程中断后，下次生成先校验全部 journal 记录、路径和目标状态：只有 `publishing` 且全部目标符合新存在状态与摘要时才确认整批新版；其他状态先校验所有必需备份，再整批恢复旧版。备份缺失、摘要不符或 journal 无效时停止，保留 journal、备份和临时文件，不在不可判定状态上继续生成。整批旧版或新版验证后才清理；清理中断后重复恢复继续收敛。online snapshot 属于同一次提交，不能先写 snapshot 再下载图标。
7. 失败或取消后清理 staging；不得修改 local supplement。

测试必须提供失败注入点，至少覆盖 registry fetch、npm pin 校验、图标下载、SVG 校验、Catalog 校验和正式替换失败。fixture 的新旧 snapshot、Catalog 与图标必须真实不同；分别在第 1、2、首个图标和末尾替换点验证普通失败整批回滚与中断恢复。覆盖首次生成原文件不存在、旧图标删除、新图标创建、无关图标保留、完整新版确认、备份损坏/缺失、无效 journal 和重复恢复；普通提交前失败后正式文件逐字节一致。

## 6. Catalog 模板

| 字段 | DeepSeek Harness（第一阶段） | Qoder（第一阶段） | ZCode（第二阶段门槛通过后） |
| --- | --- | --- | --- |
| id | `deepseek-harness` | `qoder` | `zcode-acp` |
| command / args | `dsh` / `--profile acp` | `npx` / `-y @qoder-ai/qodercli@1.1.86 --acp` | `zcode-acp-server` / 无 |
| localLaunch | 无 | `qodercli --acp` | 无 |
| installedCommand | 无 | `qodercli` | 无 |
| primaryAgentDir | `.dsh` | `.qoder` | `.zcode` |
| projectPrimaryAgentDir | `null`（回退 `.dsh`） | `null`（回退 `.qoder`） | `.agents` |
| compatibleAgentDirs | `.agents` | `.agents` | `.agents` |
| sourceKind | `local-supplement` | `official-registry` | `local-supplement` |
| integrationOrigin | `official` | `official` | `community-adapter` |
| validatedVersion | 本文验收通过后写 `0.2.1-rc.2` | 本文验收通过后写 `1.1.86` | 真实验收通过的固定版本 |
| excludedPlatforms | 空；以发布矩阵收敛 | Windows aarch64 | 以发布矩阵收敛 |
| supportsSystemPrompt | false | false | false |
| supportsExternalSessionSync | false | false | false |

`validatedVersion` 是发布证据与审计信息，不改变 native command 的实际版本。Doctor 仍验证用户本机 executable 与 ACP 握手；若协议或 Agent 明确返回实际版本，则与已验证范围比较并给出结构化版本诊断，无法获得版本时标记 unknown 并继续按真实 capability 判断，不能把 unknown 伪装成已验证或不支持。

## 7. 运行时、平台与错误契约

### 7.1 ACP client

不新增 Agent ID 分支，继续复用现有能力协商：

- `promptCapabilities` 缺失时，图片/资源按既有路径文本策略降级。
- `mcpCapabilities` 只控制声明的 MCP 能力；不得因 Agent 品牌推断远程 MCP。
- `sessionCapabilities.resume` 与顶层 `loadSession` 决定恢复策略。
- `configOptions` 继续承载模型、`thought_level` 与其他配置。
- permission / elicitation、tool、thought、usage/context 继续进入通用 ACP 事件模型。
- `AdapterInitializationData` 将 `capabilities` 与 `auth_methods` 作为同一连接初始化结果，由现有 `ConnectionInitialization` single-flight 管理。`initialize_once` 首次调用者、并发等待者与后续复用者均取得完整数据，运行实例统一从返回值设置认证方法，初始化回调不单独写入首个实例。
- 连接身份仍为 provider/workspace 与现有配置签名；认证方法随连接 generation 更新，不持久化到账户或 Agent 实例配置。复用既有 AuthRequired 一次认证、一次 `session/new` 重试边界，已配置 env 方法可自动认证，terminal 与缺失 env 返回 `provider.auth-required`。

DeepSeek 的 `reasoning_effort` 若以 `category=thought_level` 返回，复用现有档位映射；模型 value 即使是 JSON 字符串也按 ACP 原值提交，只展示 option name，不做 provider-specific 解包。

### 7.2 结构化错误

后端不得返回对客文案。优先复用现有错误码，并补齐缺失语义：

| 场景 | 错误码/契约 |
| --- | --- |
| Catalog 平台不支持 | 新增 `agent.unsupported-platform`，参数包含 Agent、OS、CPU |
| ACP 要求认证 | 复用 `provider.auth-required`，保留原始 ACP 结构化原因 |
| CLI/桥接器/底层 Agent 缺失 | 使用稳定 prerequisite 错误，参数区分 `adapter`、`agent-cli`、`runtime`，不靠 stderr 文案分类 |
| Node/npm/Agent 版本不支持 | 复用 setup/runtime 版本错误结构，参数返回 actual/required/validatedVersion |
| initialize/session 阶段超时 | 复用 `acp.doctor-timeout`，参数必须包含 phase；回收进程树 |

新增 prerequisite 错误前先检索现有错误 catalog；已有等价码时收紧参数，不建立同义错误。前端七语言根据 code/params 显示恢复动作。

### 7.3 Agent 管理与来源展示

- 不新增布局或基础控件，继续使用现有 shadcn/ui `Command + Popover` 与 Sheet。
- Agent 管理只接收当前平台可新建的 Catalog 条目；后端仍在创建命令二次校验。
- `community-adapter` 使用本地化来源标签/帮助说明，链接到 Catalog repository/website；不得仅在英文 description 写 “community”。
- env 区域明确说明内容会保存到本机配置；Qoder 帮助优先引导供应商登录，不要求用户在主路径输入 token。
- 图标只使用来源明确、允许分发的项目/官方 SVG，并在 staging 校验 HTTPS 来源和 SVG 内容；缺失或无法确认授权时阻断条目发布，不以占位图标冒充内置品牌。
- 平台、来源和凭据说明只展示帮助用户决策的信息，不解释内部字段或构建实现。

## 8. Skill 同步契约

| Agent | Gold Band 全局写入 | Gold Band 项目写入 | 读取范围 |
| --- | --- | --- | --- |
| DeepSeek | `~/.dsh/skills` | `<project>/.dsh/skills` | 主目录及对应 `.agents/skills` |
| Qoder | `~/.qoder/skills` | `<project>/.qoder/skills` | 主目录及对应 `.agents/skills` |
| ZCode（第二阶段） | `~/.zcode/skills` | `<project>/.agents/skills` | `~/.zcode/skills`、`~/.agents/skills`、项目 `.agents/skills` |

兼容目录只参与读取，不作为额外写入目标；ZCode 的项目 `.agents` 是显式 `projectPrimaryAgentDir`，因此是项目主写入目标，不是兼容目录旁路。目录解析、规范化、去重和软链接边界继续由后端统一处理。

真实验收不能只检查文件存在：写入 Skill 后必须让对应 Agent 重启或触发其正式 reload/watcher，再通过 Agent 自身能力确认 Skill 可发现。

## 9. 会话恢复与清理验收矩阵

每个 Agent 都必须覆盖同一组生命周期场景；fixture 固定通用接口契约，真实验收固定供应商实现。

| 场景 | DeepSeek Harness | Qoder 1.1.86 | ZCode 第二阶段 |
| --- | --- | --- | --- |
| new | initialize + session/new，核对模型/思考目录 | 两条 launcher 分别 initialize + session/new | 真实后端 initialize + session/new |
| prompt | 文本回合 + 至少一个真实工具调用，核对 thought/tool/usage/final | 文本、工具、权限、图片声明与 MCP | 文本、流式 diff、工具与权限 |
| stop | 发送通用 cancel，确认终态和进程仍可控 | 同左 | 同左 |
| continue | capability 声明时使用 `session/resume`，停止后可继续 | 根据 load/resume capability 恢复，并验证 close/delete/fork 清理 | 验证真实 resume 语义 |
| load-or-resume | 不声明 load 时不得调用 load | 验证 `session/list` + loadSession/history | 验证 resume/load；能力缺失时按结构化不可用处理 |
| history | 只使用 Gold Band 已持久化 timeline，不宣称外部完整回放 | 加载后核对历史顺序、工具与正文 | 核对桥接器声明的历史与 fork |
| cleanup | 优先已声明 close；确认临时会话和进程树回收 | delete/close 按 capability 清理，两条 launcher 均无残留 | close/delete/fork 临时资源与桥接进程均回收 |

任何一个 Agent 的真实验收失败只阻断该条目，不回滚其他已通过条目，也不得用 mock 成功替代真实发布证据。

## 10. 版本、engines 与平台矩阵

新增受版本控制的验证报告或生成测试 fixture，至少记录：

| 字段 | 要求 |
| --- | --- |
| Agent / adapter | 稳定 ID、包名或 native CLI 名称 |
| tested version | Catalog `validatedVersion` 对应的精确版本 |
| launcher | 本机 PATH 命令与 Registry npx 分行记录，不合并结果；本机命令需记录解析来源与实际版本 |
| Node/npm/npx | 实际版本及包 `engines` 解析结果 |
| OS / CPU | 使用 `windows/macos/linux` 与 `x86_64/aarch64` |
| authentication | 未登录、已登录、失效凭据的结果 |
| ACP lifecycle | initialize/new/prompt/stop/continue/load-or-resume/history/cleanup |
| Skill | 全局/项目写入路径及 Agent 实际发现结果 |
| result/evidence | pass/fail/blocked、日期和可复现命令/测试名 |

已知约束：Qoder npm 路径 Node >= 20，ZCode bridge Node >= 22，DeepSeek 当前实测 Node 为 22.22.3；最终矩阵以固定包 metadata 与真实运行结果为准。未知平台保持 `not-tested`，不得默认为支持。Qoder Windows aarch64 直接标记 unsupported，不要求执行无意义安装测试。

## 11. 分阶段实施步骤

### 第一阶段：通用契约、DeepSeek 与 Qoder

> 实施进度（2026-11-12）：第 2—5 项已在保持原有 11 项正式 Catalog 的前提下落地。DeepSeek Harness 固定 npm 基线 `0.2.1-rc.2` 已在 Windows x64 完成真实验收。Qoder CLI `0.2.14` 首次验收因认证、list/load、stop 与清理缺陷未通过；官方 Registry 已更新到 `1.1.86`，实现已更新 Registry snapshot、加入通用 AuthRequired 重试和受控 `launchOverrides`。1.1.86 在账号领取 Credits 后，固定 npm 与本机 PATH `qodercli` 命令两条路径均已通过完整生命周期（new、真实工具、stop、list、load/history、close/delete、进程清理）与项目、共享、全局 Skill 验收。2026-11-12 已确认本机路径以 PATH 命令为验收标准，Qoder Windows x64 真实验收完成；native 二进制被本机安全环境拦截的事实单独记录，不构成发布门槛。ZCode 桌面端 `3.17.2`（内含 CLI `zcode.cjs 0.17.1`）与桥接器 `0.82.3` 已安装；升级到桥接器 `0.84.1` 并移除手工 `ZCODE_BIN`/`ZCODE_NODE` 后，真实 GLM 后端通过包含显式 resume 与 fork 的完整生命周期和三层 Skill 验收，第二阶段门槛完成。第一阶段产物（DeepSeek local supplement、Qoder 固定 `1.1.86` 与本机 `qodercli` 覆盖）与第二阶段 ZCode（`local-supplement/community-adapter`，`0.84.1`）均已生成，正式 Catalog 为 14 项并补齐图标；统一完整回归按发布流程在三个 Agent 全部就绪后一次执行。

1. 同步产品设计文档中的 Catalog 启动配置归属、来源展示、凭据持久化和阶段数量。
2. 扩展 Catalog schema：provenance、类型化 `excludedPlatforms` 与 source 摘要。
3. 将 Catalog 生成改成 staging、完整校验、recovery journal、原子替换和失败回滚；此时仍保持现有 11 项正式输出。
4. 在 Rust 建立唯一平台规范化/判定函数，并接入 onboarding、Agent Catalog 投影与 `create_agent`。
5. 将 Agent 目录归一化改为按全局/项目作用域去重，允许 compatible 目录只与其中一个主目录相同；增加保存往返测试。
6. 使用独立测试 fixture 补 DeepSeek 当前事件契约，并执行真实工具调用/恢复验收；验收前不把候选写入正式 `BUILTIN_AGENT_IDS`。
7. 分别验证 `qodercli --acp` 与 Registry 固定 `npx ...@1.1.86 --acp`，包括登录、load/history 和 Skill；验收前不把 Qoder 加入正式精选集合。
8. 两者全部通过后，才把 DeepSeek 写入 local supplement、把 DeepSeek/Qoder 加入 `BUILTIN_AGENT_IDS`，生成并提交 13 项 Catalog 与图标。
9. 运行完整自动化、真实 Windows x64 和发布矩阵回归；失败则不发布 13 项产物。

### 第二阶段：ZCode 发布门槛

1. 在可运行真实 ZCode 的机器安装固定 bridge 版本并完成供应链核对。
2. 完成第 4.3 节与第 9 节全部真实验收，确定实际 `validatedVersion` 和平台矩阵。
3. 将 ZCode 加入 local supplement 与 `BUILTIN_AGENT_IDS`，目录配置使用全局 `.zcode` / 项目 `.agents`。
4. 重新运行完整 Catalog、onboarding、Agent 管理、Skill 与会话恢复回归。
5. 全部通过后，精选 Catalog 才从 13 增至 14。

## 12. 自动化测试与真实验收

### 12.1 自动化测试

- `node --test scripts/prepare-agent-catalog.test.mjs`
- `cargo test agent_catalog`
- `cargo test managed_agent_catalog`
- `cargo test onboarding`
- ACP client/fixture 中与 prompt、tool、resume/load、history、cleanup 相关的专项测试
- `pnpm --dir web test -- agent-management agent-icons onboarding i18n-loading`
- `pnpm --dir web exec tsc -p tsconfig.build.json`

关键接口断言：

1. 第一阶段恰好 13 项；ZCode 门槛通过后的第二阶段恰好 14 项，ID 唯一且顺序稳定。
2. Qoder 来源为 `official-registry/official`；DeepSeek 为 `local-supplement/official`；ZCode 发布后为 `local-supplement/community-adapter`。
3. source 冲突、缺失 provenance、无效平台枚举、重复平台、缺失图标或未验证版本均构建失败。
4. 任一提交前普通生成失败时，正式 snapshot、Catalog 和图标逐字节不变；最后一个替换后普通异常也回滚。中断恢复只接受整批新版或整批旧版；备份/journal 无效不修改正式文件或清理恢复证据。
5. Qoder 在 `windows/aarch64` 不进入 onboarding 和 Agent 管理 Catalog；直接调用 `create_agent(qoder)` 返回 `agent.unsupported-platform`。
6. 已配置实例仍可显示、编辑和诊断，不因当前平台限制被删除。
7. Qoder env token 输入按普通 env 持久化，UI 明确提示本机保存；日志与诊断不包含值。
8. ZCode 项目写入定位到 `.agents/skills`，不得写 `.zcode/skills`；`.agents` 在项目策略中去重、在全局策略中仍保留，Agent 管理保存往返后语义不变。
9. DeepSeek fixture 覆盖 thought、tool lifecycle、config、usage/context、resume-without-load 和无 command update。
10. Qoder fixture 覆盖 list/load/history；真实验收分别覆盖本机 PATH 命令与固定 npx launcher。
11. 所有错误返回 `{ code, params }`，前端按七语言 key 处理，不依赖英文 description 或 stderr 文本。
12. 两个运行实例使用同 provider/workspace/config 时只 initialize 一次；第二个实例收到 AuthRequired 后仍能选择共享的 env 方法，authenticate 一次并重试成功。缺少 env 时返回完整方法 ID/缺失变量名，不发送认证或重试；并发初始化者也取得相同认证方法。

### 12.2 真实验收

- DeepSeek：固定版本 Doctor；文本和真实工具回合；stop/resume；不调用 load；全局/项目 Skill 被识别；cleanup 无残留。
- Qoder 本机 PATH 命令：`qodercli --acp` 登录后完成完整生命周期。
- Qoder Registry fallback：`npx -y @qoder-ai/qodercli@1.1.86 --acp` 独立完成相同生命周期。
- Windows x64 完成第一阶段回归；其余平台按矩阵逐项记录，不把未测试写成支持。
- Windows aarch64 验证 Qoder 在两个新增入口均不可创建，后端错误稳定；DeepSeek 不受 Qoder 限制影响。
- ZCode：只有真实后端完整生命周期、Skill 和供应链验收全部通过后才执行第二阶段。

## 13. 风险与取舍

| 风险 | 处理 |
| --- | --- |
| ZCode 社区桥接器在缺失后端时长时间无 initialize | 第一阶段不发布；真实后端通过前不进入 Catalog，不靠缩短超时伪装修复 |
| Qoder Registry 固定版本与本机版本行为不同 | 两条 launcher 独立验收；Catalog fallback 只认 Registry 固定版本 |
| Qoder token 被误认为临时环境 | 明确 env 会持久化，主路径使用 `qoder login`，敏感值不进日志/遥测 |
| DeepSeek 旧说明与当前事件能力不一致 | 以固定版本真实握手和工具回合为准，旧说明仅作历史资料 |
| 多文件 Catalog 生成留下半成品 | staging 后完整校验，原子替换并在失败时回滚，逐字节失败测试固化 |
| 平台字符串跨层不一致 | Catalog 统一 `os/arch`，Rust 单一规范化和判定，Web 不自行推断 |
| Skill 路径被目录命名直觉误导 | 以 Agent 实际扫描为准；ZCode 项目只写 `.agents/skills` |
| 未测试平台被当作支持 | 验证矩阵显式区分 pass/fail/blocked/not-tested，发布范围只包含 pass |

## 14. 文档同步范围

实施前后必须同步维护：

- `docs/gold-band/产品设计文档/interaction/app/agent-management.md`：Catalog 数量、启动配置归属、provenance、平台过滤、env 持久化与来源文案。
- `docs/gold-band/产品设计文档/interaction/app/first-run-onboarding.md`：候选数量、平台过滤、第一/第二阶段准入和不自动下载边界。
- `docs/gold-band/产品设计文档/provider/overview.md`：Catalog 来源、当前客户端启动配置归属和小规模性能结论。
- `docs/gold-band/开发计划/新增流程/首次启动引导开发计划.md`：保留历史 11 项验收事实，新增后续 13/14 项扩展说明，不改写历史记录。

若实现涉及 UI 文案，七语言必须同步；`community-adapter`、本机保存 env、平台不支持和准备条件使用稳定 i18n key，不把英文 description 直接展示为唯一说明。

## 15. 方案自评审

过度设计评审：本方案没有新增 Agent 运行状态机、实例身份、缓存、队列或 provider-specific client。provenance 和平台约束是构建期小型 Catalog 元数据；统一平台函数替代跨层字符串特判；生成 staging/rollback 只解决现有多文件半写风险。ZCode 采用发布门槛而不是为未知兼容性预建 fallback，复杂度与已确认风险匹配。

性能评审：第一阶段 Catalog 从 11 增至 13、第二阶段最多 14，运行时查找和过滤均为小常数线性处理，不增加首屏网络请求、目录全量扫描、N+1、缓存或订阅范围。事务化生成只发生在构建/发版阶段；staging 会多保留一份有限 Catalog 与图标集合，但不进入 EXE 运行路径。Doctor 继续只对已准备候选使用现有有界并发和 deadline，ZCode 未准入时不会增加长等待进程。

2026-11-13 修复复核：沿用现有 journal、连接池、single-flight 与 semver 依赖，只补齐整批恢复、完整初始化结果与 provenance 校验。新增 journal 存在标志用于区分“原文件不存在”和“新版删除文件”，回滚阶段用于表达最后一个替换后仍需回滚的决定；不新增 Agent 状态机、连接身份、账户系统或兼容层。Catalog 仍固定 14 项，恢复扫描仅处理本批目标，新增摘要读取只发生在构建维护阶段；共享连接仅增加初始化认证方法的有限复制，不增加网络请求、锁覆盖范围或前端刷新。

## 16. 2026-11-13 评审问题修复验收

修复前先建立接口失败证据：

| 问题 | 失败测试与观察 |
| --- | --- |
| Catalog 部分发布未整批回滚 | `partial publication failures and crashes recover the entire previous set` 使用 `0.73.0` / `0.74.0` 的真实不同产物；第 1 个替换后失败仍保留新 snapshot，与旧集合逐字节断言不符。首次发布中断也未删除已创建目标 |
| 共享连接丢失认证方法 | `shared_connection_preserves_auth_methods_for_later_runtimes` 验证两个实例确实共享连接且只 initialize 一次；第二个实例配置了测试 token 仍返回认证必需错误，未完成 authenticate/retry |
| 缺失或无效验收版本仍可生成 | 删除官方 `validatedVersions` 条目后 `buildAgentCatalog` 未抛错；本地 `null`、空值、数字和 `latest` 也未被拒绝；生成入口的缺失记录测试出现 `Missing expected rejection` |

本次已完成验证：

- `node --test scripts/prepare-agent-catalog.test.mjs`：43 项通过，覆盖整批提交/回滚、恢复证据保留、首次发布、新旧图标、官方/本地缺失或无效版本及生成前阻断。
- `npm run web:test -- web/tests/agent-management.test.ts web/tests/agent-icons.test.ts web/tests/acp-runtime-error.test.ts web/tests/onboarding.test.tsx`：83 项通过。
- `npm run web:build`：通过；保留既有动态导入与大 chunk 提示。
- `cargo test --lib acp::connection::tests -- --nocapture`：49 项通过；`cargo test --lib agent_catalog -- --nocapture`：3 项通过；`cargo test --lib platform -- --nocapture`：13 项通过。
- `cargo test --lib acp::client::tests -- --test-threads=1`：191 项通过、6 项按设计忽略；覆盖共享连接认证方法、并发 single-flight 和 AuthRequired 重试。默认并行运行曾命中一次既有时序测试 `prompt_terminal_quiet_drain_promotes_late_system_error`，该测试单独重跑及串行全量重跑均通过。
- `cargo fmt --all -- --check`、`git diff --check`：通过。
