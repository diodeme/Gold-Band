# Direct 会话未物化恢复与失败投影修复方案

- 状态：核心设计评审通过，按三阶段交付；第一阶段“失败投影与空历史加载”已完成，第二、三阶段未开始
- 日期：2026-10-26
- 适用版本：0.20.0 后续修复版本，实际发布版本在验收后确定
- 方案定位：修复普通 Direct Attempt 在 Provider session 未物化时的恢复、创建副作用防重、身份提交、失败投影和前端加载边界；不采用针对单一现场的补丁式判断。
- 本次交付范围：第一阶段代码、接口/DOM 测试、生产构建、浏览器回归与相关文档同步；create fence、identity、`session/new`、Prompt、snapshot schemaVersion 和恢复按钮仍属于后续阶段。
- 关联文档：
  - `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
  - `docs/gold-band/产品设计文档/runtime/control.md`
  - `docs/gold-band/开发计划/acp接入/acp-ui.md`
  - `docs/gold-band/开发计划/direct模式/Direct模式完整设计与开发方案.md`
  - `docs/gold-band/开发计划/生命周期整理/工作流停止后Runtime控制与自由会话统一技术方案.md`

## 1. 结论与设计取舍

本问题不是单一空值判断错误，而是三项边界同时缺失：

1. Attempt 存在被错误等价为 Provider session 已存在，命令层提前决定 Continue。
2. 本地缺少“该 Attempt 是否已经授权过 `session/new`”的 durable 事实，无法区分从未创建与创建结果不确定。
3. 后端把“没有已物化会话”错误解释为“没有可返回详情”，导致 durable `turnError` 无法进入 UI，前端再以消息内容是否出现判断加载完成。

新方案做以下修复：

1. 在现有 `acp.snapshot.json` 中增加最小创建副作用栅栏 `ProviderSessionCreateFence`：只有 `Open` 可以为普通 Direct Attempt 授权一次 `session/new`；授权前原子转换为 `Sealed { operationId }`。缺失、损坏或未知格式统一解析为 `Unknown`，永不自动创建。
2. Provider session attachment 的 canonical identity 是 `AdapterConnectionKey + connectionGeneration + ACP sessionId`；snapshot 保存跨进程可恢复的 durable reference，重启后必须重新 attach 才能取得新的 canonical attached identity。worker-ref 只是可重建投影，不再参与“会话是否存在”的双重裁决。
3. 桌面命令层只提交 Direct Prompt intent，不再因 Attempt 已存在就构造 Continue。最终目标在 Attempt 的 Prompt 串行执行通道内解析为 `Continue`、`CreateDirectSession` 或 `Blocked`。
4. Provider 传输目标与 Prompt 内容语义分离。普通 Direct 恢复创建使用 `DirectRecoveryBootstrap`，只组合冻结的原始 Direct 用户输入与本轮 durable `promptSubmission`，不注入 Runtime hidden/profile/system 内容；两次用户输入在 UI timeline 中各保留一个 canonical 事件。
5. 扩展现有 snapshot metadata transaction，使 lifecycle、create fence、durable session reference、Runtime control 和 Provider metadata 通过同一文件锁、最新值重读、owner/revision 校验与原子写入维护；禁止调用方用旧内存对象整份覆盖新事实，不另建平行事务框架。
6. `session/new` 前先完成可失败的本地准备与 initialize，最后复核 Stop / owner，再将栅栏从 `Open` 封闭并取得有类型 permit；调用创建成功后立即提交 durable reference，再写 worker-ref、timeline 和发送 `session/prompt`。
7. 后端返回 Attempt 级 ACP 投影。只要 Attempt 的 snapshot 存在，即使 unavailable、没有 sessionId、timeline 为空，也返回 lifecycle 和结构化 `turnError`。
8. 前端分别管理查询状态、会话物化状态、历史空或非空、Runtime lifecycle；成功查询到空 VM 即进入 `ready-empty`，删除约 32 秒的内容驱动轮询。
9. `Sealed + no durable reference` 和 `Unknown + no durable reference` 不在旧 Attempt 上重置栅栏。桌面提供“重新发起会话”，显式创建新 Attempt，保留旧 Attempt 的证据与失败状态。

能力边界：ACP schema 1.6 的 `NewSessionRequest` 没有客户端可依赖的幂等键；`_meta` 也不保证 Provider 采用。因此客户端不能同时保证“创建响应丢失后自动恢复”和“远端只创建一次”。本方案选择不自动重复外部创建副作用，并把不确定状态明确投影给用户。

## 2. 背景、证据与根因

### 2.1 现场链路

用户停止 Direct 首次执行，重新打开历史会话并继续发送，最终看到：

> ACP 会话失败，无法获取当前会话，未获取到具体原因。

现场材料与当前代码共同支持以下链路：

1. 首次执行在 initialize 阶段停止，Run / Node 进入可继续暂停；本地没有有效 worker-ref、sessionId 和 timeline。
2. 桌面 `existing_attempt_prompt_session_target` 将既有 Attempt 一律解释为 Continue。
3. ACP 公共入口返回 `acp.session-restore-reference-missing`。
4. durable `turnError` 已落盘，但后端将 unavailable、无 sessionId、无可展示 timeline 的快照提前返回 `None`。
5. 前端没有接受该失败详情，经过约 32 秒有界重试后显示通用缺会话原因。

“现场 Raw 未见 `session/new`”只能解释该次现场，不能证明所有同类记录都从未向 Provider 发出创建请求。当前写入顺序允许请求已经发送而 Raw 尚未记录，也允许 Provider 已创建但本地尚未持久化 identity。

### 2.2 已核实的代码证据

以下行号是方案分析时的工作区定位参考，实施时必须按符号重新定位。

| 证据 | 当前实现位置 | 含义 |
|---|---|---|
| 既有 Attempt 一律 Continue | `src-tauri/src/commands.rs:141` | Attempt 存在被错误等价为 Provider session 可恢复 |
| 普通追问固定 Continue + UserMessage | `src/app/mod.rs:5783` | 只增加 SessionMode 分支无法完成正确恢复 |
| 普通追问准备接口清空首轮输入区段 | `src/app/mod.rs:5828` | 恢复创建不能沿用普通追问的数据裁剪策略，但 Direct 仍不得重新引入 Runtime hidden context |
| RawAgent New 读取 requirement，UserMessage 的 requirement 为空 | `src/provider/mod.rs:2380` | 直接改成 New + UserMessage 会丢失本轮实际正文 |
| Prompt lock 在 `run_prompt_inner` 内获取 | `src/acp/client.rs:3095` | 锁外预判与锁内执行不能形成同一个创建授权边界 |
| session setup 负责创建与恢复 | `src/acp/client.rs:4480` | 最终目标与创建 permit 应在该执行通道上游统一决定 |
| `session/new` 后继续执行配置与记录 | `src/acp/client.rs:4674` | identity 提交过晚会扩大远端成功、本地未知的窗口 |
| 先创建传输请求，后记录 outbound Raw | `src/acp/client.rs:5836` | Raw 缺失不能作为从未发送的否定性证据 |
| unavailable 空会话被提前返回 `None` | `src-tauri/src/view_models.rs:4150` | durable 失败原因无法进入前端 |
| 前端按内容缺失进行有界重试 | `web/src/components/acp/ACPChatDialog.tsx:4495` | 查询完成与有消息内容被错误绑定 |
| ready 判定要求历史事件 | `web/src/components/acp/ACPChatDialog.tsx:12390` | 真实空历史和详情尚未加载无法区分 |

### 2.3 根因归类

**根本性设计缺陷：**

- Attempt identity、Provider session identity 和 worker-ref 投影的职责没有分清，命令层以 Attempt 存在推断 Continue。
- 缺少外部不可逆副作用的本地发送权事实，无法安全决定是否再次 `session/new`。
- Provider 传输模式与 Prompt 内容语义耦合，导致恢复目标改变时冻结的原始 Direct 输入与当前消息不能按正确契约组合。
- snapshot 存在多个整对象写入者，晚到写入可能清除 identity、错误或控制状态。

**好设计上的实现不完整：**

- 已有 Attempt Prompt 串行通道、owner / revision、原子文件写入和结构化 `turnError`，但最终目标解析、创建授权和 identity 提交没有统一放入这些机制。
- 后端已经持久化失败事实，但查询投影和前端 readiness 没有消费。

**不能在本地消除的分布式边界：**

- 请求可能已到达 Provider，而响应未到达或未持久化。本地不能通过文件缺失、日志缺失、固定延时或重新 initialize 证明远端没有创建。

因此必须修复身份、状态与副作用边界，不能用“缺 worker-ref 就 New”“Raw 中没有 `session/new` 就 New”或延长前端轮询解决。

## 3. 目标、范围与用户行为

### 3.1 目标

1. 可证明栅栏仍为 `Open` 的普通 Direct Attempt，可以在下一次用户发送时原地完成首次 Provider session 创建。
2. 已经或可能授权过创建、但没有 durable reference 的 Attempt，绝不自动重复 `session/new`。
3. 已有合法 durable reference 的 Attempt 始终 Continue；派生 worker-ref 缺失时可以在写路径重建，并在当前连接代重新 attach。
4. 所有失败 Attempt 都能立即显示结构化原因，不依赖 timeline 中先出现消息。
5. Stop、并发发送、迟到响应、重启和写入失败下，创建次数、identity 与 lifecycle 保持单调一致。
6. Direct 修复不放宽 Dynamic、Workflow、AUTO 等模式的严格恢复规则。

### 3.2 用户可见行为

| 场景 | 后端决策 | 用户结果 |
|---|---|---|
| 普通 Direct 首次执行在创建前停止，fence 为 Open | 下一次发送可解析为 `CreateDirectSession` | 原 Attempt 原地恢复并进入对话 |
| snapshot 与 canonical 配置能构造合法 durable reference，worker-ref 缺失 | 修复投影并重新 attach 后 Continue | 原会话继续，不重复创建 |
| fence 为 Sealed、无 durable reference | Blocked，禁止再次 New | 展示创建结果不确定，提供“重新发起会话” |
| fence 为 Unknown、无 durable reference | Blocked，禁止推断式 New | 展示旧数据或状态无法确认，提供“重新发起会话” |
| identity 冲突或 snapshot 损坏 | Blocked，保留现场 | 展示具体结构化错误 |
| unavailable、timeline 为空、存在 turnError | 返回 Attempt VM | 首次查询即显示失败，不再等待约 32 秒 |
| Direct 健康空会话 | 返回 ready-empty | 显示正常空态或 composer，不伪造消息 |
| Workflow/AUTO 初始化失败或中断 | 保留各自 shell 语义 | 不因 Direct 修复显示普通聊天 composer |
| Dynamic 或严格 Runtime 缺恢复引用 | 保持严格失败 | 不自动创建 Direct session |

### 3.3 不扩大到的事项

- 不新增会话 aggregate、第二套 lifecycle、独立创建状态文件、通用重试平台、消息队列或后台扫描服务。
- 不根据 Provider 名称或诊断字符串实现特例。
- 不修改 Workflow/AUTO 的业务调度状态机，不给 Dynamic inner Attempt 开放普通 Direct 的首次创建资格。
- 不为历史数据进行全量迁移，也不扫描全部 Raw、timeline 或 usage 补造事实。
- 第一阶段只修改失败投影与空历史加载；Provider 创建副作用和恢复语义留到第二阶段，并在各阶段实现验证后同步对应产品设计文档和关联开发计划。

### 3.4 三阶段交付边界

为降低跨 ACP、Runtime、Tauri 和 React 的一次性交付风险，本方案按以下阶段实施；阶段边界不改变最终正确性目标：

1. **第一阶段：失败投影与空历史加载。** 后端在 snapshot 已存在且包含可查询 lifecycle / 合法 `turnError` 时，即使 unavailable、无 sessionId、timeline 为空也返回 Attempt VM；前端把非 active 的真实空历史视为 `ready-empty`，删除内容驱动重试，同时保留 `eventPage.total > 0` 的分页加载。此阶段不修改 Provider 副作用路径。
2. **第二阶段：Direct 安全恢复闭环。** 实现 create fence、串行通道内目标解析、durable submission、DirectRecoveryBootstrap、迟到 durable reference 提交和显式新 Attempt 恢复。
3. **第三阶段：snapshot / identity 基础设施治理。** 在第二阶段确有需要的边界上完成 schemaVersion、全量 writer 收口和类型传播；不得为了本次 Bug 一次性重写无关存储路径。

第一阶段明确不包含 create fence、snapshot schemaVersion、identity 模型调整、`session/new`、Prompt 内容、“重新发起会话”按钮或 snapshot writer 重构。第一阶段验收通过后再进入第二阶段，不能把第二阶段的部分副作用逻辑提前混入。

## 4. 数据归属与权威事实

| 数据 | 所属领域与职责 |
|---|---|
| Task definition、Run snapshot、requirement、inputs | 冻结的原始需求、Agent 配置和输入来源 |
| Run / Round / Node / Attempt | canonical locator、Runtime lifecycle、控制权和继续资格 |
| `acp.snapshot.json` | ACP lifecycle、create fence、跨进程 durable session reference、turnError、Provider metadata |
| durable Provider session reference | 由 snapshot 中的 sessionId、adapter/workspace 绑定与 canonical Run 配置共同校验得到；用于重启后恢复 attach |
| attached Provider session identity | `AdapterConnectionKey + connectionGeneration + ACP sessionId`，只在当前进程连接代内作为 canonical attachment identity |
| worker-ref | 从 durable reference 与冻结配置生成的可恢复投影 |
| timeline / prompt usage | 用户和 Agent 的可见历史、统计与去重事实 |
| Raw / diagnostics | 审计和故障定位，不参与“从未创建”的证明 |
| Prompt Queue | 待发送输入和顺序，不保存另一套目标或 identity |

关键不变量：

1. Attempt 存在不代表 Provider session 存在。
2. durable Provider session reference 存在且校验通过时，目标只能是 Continue；执行前必须在当前连接代完成 attach，得到 canonical attached identity。
3. 无法构造 durable reference 时，只有 fence 明确为 Open、没有矛盾的 Provider 正向事实且普通 Direct 资格成立，才允许创建。
4. worker-ref 不得反向覆盖或否定 snapshot 中的 durable reference；旧 attached identity 也不得跨 connection generation 复用。
5. lifecycle、create fence、session identity 分别表达不同事实；不得互相推断或复制成第二套状态机。
6. timeline、Raw 或 diagnostics 的缺失只能表示本地没有相应记录，不能证明外部副作用未发生。

## 5. Snapshot 事务与写入收口

### 5.1 创建副作用栅栏

在现有 snapshot 中增加有类型字段 `providerSessionCreateFence`。持久化类型与读取后的领域视图必须分开，避免未知字段导致整个 snapshot 无法读取：

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum StoredProviderSessionCreateFence {
    Open,
    Sealed {
        #[serde(rename = "operationId")]
        operation_id: String,
    },
}
```

读取后的领域视图为：

```text
Open
Sealed(operationId)
Unknown(reason)
```

语义：

- `Open`：新版本在真正创建普通 Direct Attempt 时可靠写入，表示尚未授权过该 Attempt 的首次 `session/new`。
- `Sealed { operationId }`：本地已提交一次创建发送权。Provider 可能尚未收到、正在处理、已经创建，或响应已经丢失。
- `Unknown`：字段缺失、null、未知变体、格式损坏或高版本格式无法解释。`Unknown` 是读取和错误投影结果，不作为可写的正常初始值，也绝不降级为 `Open`。

`StoredProviderSessionCreateFence` 不直接作为整个 `AcpSessionMetadata` 的派生反序列化字段。读取必须先保留顶层 JSON envelope，再对 fence 做字段级解析，得到 `ProviderSessionCreateFenceView`。fence 局部损坏不能阻止 lifecycle、durable session reference、`turnError` 和 revision 的独立读取；只有 JSON 文件本身无法解析或 canonical Attempt 无法识别时，才返回 snapshot / query 级结构化错误。

状态转换只允许：

```text
新普通 Direct Attempt创建 -> Open
Open --seal(operationId)--> Sealed(operationId)
Sealed -> Sealed
Unknown -> Unknown
```

取消、失败、新 turn、队列重投、重启和普通 metadata 更新都不能重新打开栅栏。用户显式恢复通过创建新 Attempt 获得新的 `Open`，不修改旧 Attempt。

### 5.2 Snapshot envelope 与版本规则

`acp.snapshot.json` 新增顶层 `schemaVersion`，本方案落地版本为 `1`。它只描述 snapshot 文档结构，不等同于 Runtime node 上用于存储迁移协调的 `acp_storage_schema_version`。

读取规则：

1. 缺少 `schemaVersion` 的现有文件按 legacy v0 读取；已知旧字段继续解析，但缺失 create fence 必须得到 `Unknown(Missing)`，不能补写 `Open`。
2. `schemaVersion = 1` 按当前 envelope 和字段级 parser 读取。
3. 大于当前版本的文件允许提取 canonical Attempt locator 和可安全识别的查询级诊断，但所有 mutation 必须拒绝，不能由旧代码降级覆盖。
4. `schemaVersion` 类型错误、负数或 envelope 损坏时返回结构化诊断，并拒绝写入。
5. 第一次合法 mutation 可以把可完整理解的 legacy v0 提升为 v1；升级时必须保留未知顶层字段。无法完整理解的数据不得借升级重写。

### 5.3 扩展现有 metadata transaction

不新增平行 snapshot 事务层。实施必须复用并收紧现有 `session_metadata_lock`、`patch_session_metadata`、owner/revision CAS 和 `write_json` 原子替换路径。事务必须：

1. 在 blocking worker 上进入短锁，读取最新 raw envelope 与 revision，而不是接收调用方持有的旧完整对象。
2. 校验 snapshot schema version、canonical Attempt、mutation 所需的 owner / operationId 和允许的状态转换。
3. 只应用有类型 mutation，并保留未参与修改的 lifecycle、fence、durable reference、turnError、Runtime control、Provider metadata 和未知顶层字段。
4. 使用现有原子写入能力提交完整的新版本；写入失败时不对外宣称转换成功。
5. 对未知高版本或无法安全理解的数据拒绝写入，避免旧代码整份重写并丢弃新字段。
6. 返回提交后的 revision 和有类型结果，供后续副作用使用。

建议最小 mutation 集合：

```text
InitializeDirectCreateFence
SealDirectCreateFence(operationId)
CommitProviderSessionReference(createOperationId, validatedDurableReference)
UpdateLifecycle(expectedOwner, transition)
UpdateRuntimeControl(expectedRevision, controlMutation)
RecordTurnError(expectedOwner, errorInfo)
UpdateProviderMetadata(metadataMutation)
```

具体命名服从现有模块。现有 `write_session_metadata*` 一类从 typed object 重建完整 JSON 的入口必须删除、私有化或改为调用上述 mutation，不能只合并 lifecycle 后覆盖未知新字段。title、config、terminal settlement、repair 和 legacy normalization 等 snapshot 写路径都必须迁移到该现有事务，或明确证明只写独立文件。

### 5.4 初始化时机

1. 仅在真正分配新的普通 Direct Attempt 时写入 `Open`，并先于 Provider 连接、initialize 和任何 `session/new`。
2. 历史读取、缺文件补建、发送重试、进程重启、repair 和 VM 查询不得补写 `Open`。
3. 新 Attempt 的 canonical 元数据与 `Open` 必须在现有创建边界中可靠提交；提交失败时不得发布一个可继续执行的半成品 Attempt。
4. 旧数据没有该字段时保持 `Unknown`，不做推断式迁移。

## 6. Provider session reference 与 attached identity 归一化

本方案区分两个生命周期不同的类型：

```text
ValidatedDurableProviderSessionReference
  = canonical Attempt locator
  + provider / adapter workspace binding
  + canonical provider cwd / config binding
  + ACP sessionId

AttachedProviderSessionIdentity
  = AdapterConnectionKey
  + connectionGeneration
  + ACP sessionId
```

snapshot 只持久化或引用构造 durable reference 所需的 restart-stable 事实；`connectionGeneration` 不跨进程持久化。Continue 执行时先校验 durable reference，再在当前连接代 restore / attach，取得 canonical attached identity。现存 `acpSessionId`、`sessionId` 别名和 worker-ref 必须通过一个归一化入口解析，不能由调用方分别取值。

归一化规则：

1. 每个 sessionId 候选值都先验证类型、trim 后非空；随后结合 canonical Attempt、Provider、adapter workspace、project/workspace 和 provider cwd 配置构造 durable reference。
2. 同一对象内两个非空别名不同，返回 `SessionIdentityConflict`，不能挑一个看似较新的值。
3. snapshot 与 worker-ref 中的非空 identity 不同，返回冲突，不能由时间戳或文件顺序裁决。
4. snapshot 能构造合法 durable reference、worker-ref 缺失时，在用户发送的写路径重建 worker-ref，再在当前连接代 attach 后 Continue。
5. 旧 worker-ref 有可信 sessionId、旧 snapshot 确实缺 reference 时，可在事务接口内结合 canonical 配置补齐 snapshot；查询路径只报告和投影，不执行维护写入。
6. Provider cwd 从 canonical Run / worktree 冻结配置解析，不能把 snapshot 中可能表示 Attempt 目录的路径直接当作 Provider cwd。
7. 非法、矛盾或归属不明的引用返回结构化错误，绝不能因为恢复失败而回退为 New。
8. ACP Continue 校验、setup、VM 和摘要投影必须消费同一个 `ValidatedDurableProviderSessionReference`；只有实际 transport/session runtime 消费 `AttachedProviderSessionIdentity`。
9. connection generation 变化使旧 attached identity 失效，但不清除 durable reference；必须重新 attach，不能把旧 route 或旧 runtime 当作仍然有效。

worker-ref 只包含恢复 Provider 所需的派生数据。它写入失败不会改变 durable reference 已经存在的事实；下一次发送修复它并 Continue，而不是重新创建 Provider session。

## 7. 调用意图、目标解析与模式隔离

### 7.1 命令层只提交意图

桌面命令层不再执行以下判断：

- Attempt 已存在，因此 Continue；
- worker-ref 缺失，因此 New；
- Raw 中没有创建记录，因此 New；
- 当前 timeline 为空，因此这是首轮。

普通 Direct 发送入口只提交 `DirectPromptIntent`，其中包含 canonical locator、durable admission 的 `turnId + lifecycle operationId` 和经过现有权限校验的调用来源。正文、引用、附件和角色不得从 React 草稿、IPC 闭包参数或锁外提前生成的对象继续传入执行阶段；最终内容准备必须按 turn identity 重新读取 snapshot 中已持久化的 `promptSubmission`。命令层不传入一个自行推断的 `SessionMode`。

Dynamic、Workflow、AUTO 或 Runtime 已明确指定的严格 New / Continue 继续使用各自有类型 intent，不与普通 Direct 的恢复资格合并。

### 7.2 最终目标解析

最终解析发生在 `run_prompt_inner` 获取既有 Attempt Prompt 串行执行通道之后，并重新读取最新 snapshot、owner 和 canonical Runtime 状态。

解析结果：

```text
Continue(ValidatedDurableProviderSessionReference)
CreateDirectSession(EligibleDirectCreateContext)
Blocked(RuntimeErrorInfo)
```

普通 Direct 的规则顺序：

1. 能构造合法 durable reference：`Continue`，与 fence 值无关；若 worker-ref 缺失，先修复投影，随后在当前连接代 attach。
2. identity 冲突、损坏或归属不合法：`Blocked`。
3. 无 durable reference，且不是当前 canonical 普通 Direct Attempt、已被替代、只读、Dynamic inner Attempt、Workflow/AUTO、或 Runtime 不允许发送：`Blocked`。
4. 无 durable reference，fence 为 `Open`，但存在与“从未创建”矛盾的正向事实：`Blocked(SessionCreationEvidenceConflict)`。
5. 无 durable reference，fence 为 `Open`，没有矛盾事实且生命周期允许用户发送：`CreateDirectSession`。
6. 无 durable reference，fence 为 `Sealed`：`Blocked(SessionCreationUncertain)`。
7. 无 durable reference，fence 为 `Unknown`：`Blocked(SessionCreateFenceUnknown)`。

矛盾事实只用于否决，永不用于授权。允许使用的证据包括已校验的 worker-ref、Provider 派生的 assistant/tool timeline 摘要、Provider usage、已建立 session 的 metadata 或当前执行已经持有的明确创建发送回执；本地尚未发送的 user timeline item 不构成 Provider session 已创建的证据。不得为了证明可以创建而扫描全部 Raw、timeline 或 usage；任何证据缺失都不能把 Unknown / Sealed 推回 Open。

目标解析是瞬时执行计划，不持久化第二份 mode。锁外可以做只读预检查以尽早返回明显错误，但任何预检查结果都不能直接授权 `session/new`。

### 7.3 模式隔离

- 只有普通 Direct 的当前 canonical Attempt 能从 `Open` 取得创建 permit。
- Dynamic inner Attempt 缺 identity 时保持严格恢复失败，不因外层 Direct 而自动创建。
- Workflow/AUTO 的初始化、新建和恢复继续由其调度状态机明确决定。
- manual check、历史只读、非 current Attempt 和已终结不可继续状态不能取得 permit。
- Provider 自身已有明确 New 调用的路径仍需使用现有 metadata transaction，但不复用 Direct 的“恢复时可首次创建”资格。

## 8. Provider 传输与 Prompt 内容语义分离

### 8.1 两个正交维度

Provider 传输目标只表达：

```text
New(CreateSessionPermit)
Continue(AttachedProviderSessionIdentity)
```

Prompt 内容语义只表达：

```text
DirectInitial
DirectFollowUp
DirectRecoveryBootstrap
```

目标解析先产生 `ValidatedDurableProviderSessionReference`，setup 在当前连接代 restore / attach 后才形成 transport 使用的 `AttachedProviderSessionIdentity`。`SessionMode` 不再隐式决定正文取 requirement 还是当前消息。调用者必须在最终目标解析后明确选择内容语义，Provider adapter 只接收已渲染的请求。本节仅冻结普通 Direct 的语义；RuntimeManaged、Workflow、AUTO 和 Dynamic 继续遵守各自 prompt 契约，不能复用 Direct bootstrap。

### 8.2 内容契约

**DirectInitial**

- 用于正常新 Attempt 的首次执行。
- 从冻结的 Direct authoring / task-prompt input 构造用户原始输入、引用与附件。
- system prompt 保持空；不注入 Runtime hidden context、profile、goal/task 包装、predecessor、output contract、repair 或 AI-DYNAMIC 内容。
- timeline 按 Direct 首轮产品规则投影一个 canonical user event。

**DirectFollowUp**

- 用于已有合法 durable reference 的普通追问。
- 只发送从当前 durable `promptSubmission` 读取的用户输入、引用和本轮附件。
- system prompt 保持空；不重复注入原始 Direct 输入，也不增加 Runtime hidden context。

**DirectRecoveryBootstrap**

- 用于普通 Direct Attempt 的 fence 仍为 `Open`，但本次用户发送才真正创建 Provider session。
- 原始部分只能来自该 Attempt 创建时冻结的 Direct authoring / task-prompt input；当前部分只能来自本轮 durable `promptSubmission`，两者都保留用户原文、引用和附件身份，不从最新 Task definition、可变草稿或锁外闭包回填。
- 使用专用模板明确区分“初始化中断前的原始用户输入”和“当前用户输入”，并声明当前输入具有本轮指令优先级；该模板只是重建缺失 Provider 对话上下文，不把原始输入伪装成 system prompt。
- system prompt 仍为空，不注入 Runtime hidden context、profile、goal/task 包装、predecessor、output contract、repair、AI-DYNAMIC 或自动记忆正文。
- 若冻结的原始 Direct 数据或当前 durable submission 无法完整取得，返回结构化错误，不用空字符串、最新 Task definition 或 UI 当前草稿替代。
- UI timeline 必须让两次用户输入各出现一个 canonical user event：原始事件不存在时先按其稳定 prompt/event identity 幂等补建，再记录当前事件；已存在时只复用，不重复插入。Provider bootstrap 的包装标签不进入用户气泡。

### 8.3 准备接口

`run_prompt` 接收 intent 和一个按已解析目标准备内容的 factory。factory 输入只包含已校验 locator、turn identity 和解析结果，并在串行通道内重新读取 durable 数据：

```text
run_prompt(intent, prepare_for_resolved_target, ...)
  -> 进入 Attempt Prompt 串行通道
  -> 解析 Continue / Create / Blocked
  -> 按最终结果准备 DirectFollowUp 或 DirectRecoveryBootstrap
  -> 执行创建或恢复
```

目标在等待串行通道期间由 Create 变为 Continue 时，必须重新选择 `DirectFollowUp`；不能复用锁外提前拼好的 bootstrap。附件解析、workspace 文件和权限配置也按最终 durable submission 与内容语义准备并进行请求体测试。

### 8.4 Prompt 管理

如实施需要新增或修改 DirectRecoveryBootstrap 模板：

1. 必须先遵守 `docs/gold-band/rules/prompt-authoring.md`。
2. 模板统一放在 `src/prompts/`，不得把长文本硬编码在 Rust 实现中。
3. 除规则明确豁免的提示词外，同步维护 `zh-CN`、`zh-TW`、`en`、`ja-JP`、`ko-KR`、`pt-BR`、`es` 相同相对路径。
4. Direct system prompt 继续严格为空；DirectRecoveryBootstrap 模板及原始 / 当前两段用户输入只渲染到 user prompt，不把任何稳定运行时上下文转移到 system prompt。

## 9. 创建、身份提交与 Prompt 副作用顺序

### 9.1 正常顺序

普通 Direct 首次恢复创建按以下顺序执行：

```text
进入 Attempt Prompt 串行通道
-> 读取最新 snapshot 并解析为 CreateDirectSession
-> 从冻结 Direct 输入与当前 durable promptSubmission 准备 DirectRecoveryBootstrap、附件和 Provider 配置
-> 建立连接并完成 initialize
-> 短事务最后复核 owner、Stop、Runtime 资格、durable reference 和 fence
-> Open -> Sealed(createOperationId)，返回 CreateSessionPermit
-> 使用 permit 最多调用一次 session/new
-> 校验响应并立即提交 durable Provider session reference
-> 写入或修复 worker-ref 投影
-> 幂等确保原始与当前两个 canonical user timeline event 均存在
-> 再次确认本轮仍允许继续
-> 调用 session/prompt
-> 按现有流程提交输出、usage 和 terminal lifecycle
```

关键约束：

1. Prompt / 附件准备和 initialize 可以失败，但不得提前封闭 fence；这些步骤不能发送 `session/new`。
2. 最后复核与 `Open -> Sealed` 在同一短事务中完成。失败时不返回 permit，底层 adapter 的 `session/new` 调用次数必须为 0。
3. `createOperationId` 复用取得 permit 的 durable admission lifecycle operationId，并永久记录在 Sealed fence 中；permit 绑定 canonical Attempt、该 createOperationId、Provider 配置摘要和提交 revision，只能由当前执行调用一次。底层 transport 不得对 `session/new` 做透明自动重试。
4. fence 一旦 Sealed，即使进程在真正发送前崩溃也保持 Sealed。客户端无法证明请求未到达，不能重新打开。
5. 合法 sessionId 返回后，先用专门事务提交 durable reference，不等待标题、完整 timeline、usage、Raw 或 worker-ref。
6. durable reference 提交失败时不发送 `session/prompt`，保留 Sealed 并记录结构化持久化错误。
7. durable reference 已提交后，worker-ref 或 timeline 写入失败不能导致再次 New；后续从 durable reference 继续修复并重新 attach。
8. Raw 记录是审计副作用，不作为 permit 消耗或 durable reference 成立的条件。

### 9.2 durable reference 提交事务

`CommitProviderSessionReference` 必须：

- 校验 canonical Attempt 和 fence 为相同 createOperationId 的 Sealed，或校验已存在相同 durable reference 后幂等成功。
- 拒绝把同一 Attempt 的已知 durable reference 改成另一个值。
- 只更新 durable reference 与必要 restart-stable Provider metadata，保留最新 lifecycle、Stop、owner、turnError 和 Runtime control；不得持久化旧 connection generation。
- createOperationId 的校验独立于当前 lifecycle owner。创建响应在 Stop 之后迟到时，旧 owner 即使已失效也允许提交相同 fence 对应的 durable reference，但不得把 cancelled / paused 生命周期改回 running。
- 如果当前 lifecycle 已进入后续 turn，仍只允许相同 createOperationId 对空 reference 或相同 reference 做幂等提交；任何冲突都返回结构化错误，不能按时间戳覆盖。
- 提交失败时返回稳定错误 code 和 diagnostic，不包含对客文案。

### 9.3 正常 Continue

正常 Continue 不读 create fence 来决定是否继续，不新增 fence 写入，也不构建 DirectRecoveryBootstrap。它只消费已校验 durable reference、在当前连接代取得 attached identity、准备 DirectFollowUp、修复必要的 worker-ref 投影并执行 `session/prompt`。

## 10. Stop、并发、迟到响应与 Prompt Queue

创建 fence 的封闭提交是本地 `session/new` 发送权的线性化点：

- Stop 先提交：最后复核失败，fence 保持 Open，`session/new = 0`。
- seal 先提交：当前执行最多允许一次 `session/new`；之后到达的 Stop 不能保证撤回已经获得的外部发送权。
- Stop 后返回合法 sessionId：仍按 fence createOperationId 提交 durable reference，但不发送旧 turn 的 `session/prompt`，也不复活旧 lifecycle。
- 新 turn 在旧创建请求收敛前到达：通过既有 Attempt Prompt 通道等待；进入后重新解析 identity / fence，不能沿用锁外 Create 结果。
- 两个并发发送：只有一个事务能把 Open 转为 Sealed；另一个看到 identity 后 Continue，或在 identity 尚未提交时得到创建进行中 / 结果不确定的有类型状态，不能再次 New。
- 应用重启：原样读取 Open、Sealed 或 Unknown，不做补写和重开。

Prompt Queue 继续只负责输入顺序：

1. 每个排队 turn 通过同一 admission 和最终目标解析入口执行。
2. queue retry、进程重启和重复投递不能重置 fence 或重复创建。
3. 创建成功不等于用户 Prompt 成功；`session/prompt` 失败时保留 identity，下一轮 Continue。
4. 显式创建新 Attempt 时不暗中迁移或批量重放旧 Attempt 的 queue；未发送草稿由 UI 按现有草稿规则保留。

锁范围要求：Attempt Prompt 串行通道可以覆盖一次完整执行；snapshot 文件锁只覆盖读取、校验和原子提交，不能在其中等待 initialize、网络响应、附件转换或模型输出。

## 11. 故障窗口矩阵

| 故障位置 | durable 事实 | 下一次行为 |
|---|---|---|
| prepare / initialize 前或期间停止、失败或崩溃 | Open、无 durable reference | 普通 Direct 资格仍成立时允许再次尝试 |
| 最后复核发现 Stop 或 owner 变化 | Open、无 durable reference | 不创建；按最新 lifecycle 投影 |
| seal 写入失败 | Open 或原状态 | `session/new = 0`，修复存储后重新解析 |
| seal 成功、请求实际发送前崩溃 | Sealed、无 durable reference | 不自动 New，提供显式重新发起 |
| Provider 创建成功、响应丢失 | Sealed、无 durable reference | 不自动 New，显示创建结果不确定 |
| 收到 sessionId、durable reference 提交失败 | Sealed、无 durable reference | 不发送 Prompt，显示持久化失败 |
| durable reference 已提交、worker-ref 失败 | 有 durable reference | 修复投影并重新 attach 后 Continue |
| durable reference 已提交、timeline 记录失败 | 有 durable reference | 按现有 turn 去重 / repair 契约恢复，不再 New |
| worker-ref 成功、`session/prompt` 未发送 | 有 durable reference | 后续重新 attach / Continue |
| Stop 后创建响应迟到 | 有 durable reference，lifecycle 保持停止结果 | 下一轮 Continue，不复活旧 turn |
| 旧 metadata / config 写入迟到 | 最新事务 revision 已前进 | 拒绝或合并，不清除 fence / durable reference / Stop |
| 旧格式缺 fence、无 durable reference | Unknown | 不自动 New，显式新 Attempt |
| fence 损坏或未知格式 | Unknown + diagnostic | 不写回默认值，显示结构化错误 |
| snapshot 与 worker-ref identity 冲突 | 冲突事实保留 | 阻止发送，等待显式处理 |
| fence 为 Open，但已有 Provider 派生的建立证据 | EvidenceConflict | 只阻止 New，不反向补造 identity；展示结构化错误 |

## 12. Attempt 级失败投影与前端加载

### 12.1 后端投影

后端查询以 canonical Attempt 为单位构造 ACP ViewModel，而不是以“已存在 sessionId 或 timeline”为返回条件。

只要 Attempt 的 snapshot 存在且可识别，就返回至少以下事实，并在 Rust / TypeScript 两端冻结同名 DTO：

```text
AcpSessionVm {
  attempt locator
  snapshotRevision: u64
  lifecycle / availability
  materialization: materialized | unmaterialized | unknown
  historyState: pending | empty | non-empty | failed
  eventPage: { total, ...existing cursor fields }
  turnError?: RuntimeErrorInfo
  recoveryAction: none | restart-attempt
  recoveryReasonCode?: stable error code
}
```

约束：

1. unavailable、无 durable reference、timeline 为空时也返回 VM。
2. 合法 `turnError` 原样以 `code + params + recovery + diagnostic` 投影；后端不生成对客文案。
3. fence 或其他局部字段损坏时，使用 raw envelope 中仍可识别的 locator、revision、lifecycle 和 `turnError` 返回 Attempt VM；JSON envelope 本身损坏或无法识别 canonical Attempt 时才使用查询级结构化错误。
4. `None` 只表示目标确实不存在或调用方没有权限，不再表示“存在但未物化”。
5. `eventPage.total > 0` 但首批 events 尚未加载时返回 `historyState=pending`；`total = 0` 才是 `empty`，不能用空数组冒充真实空历史。
6. 查询路径不顺便重建 worker-ref、补 fence 或清理错误。
7. `materialization` 只能由合法 durable reference 或明确的 Provider 建立事实派生；本地 admission / user timeline event 单独存在时仍是 `unmaterialized`，不能继续沿用“任意非空 timeline 即 sessionEstablished”的旧规则。

### 12.2 前端状态拆分

前端必须分别表示四个维度：

| 维度 | 示例状态 |
|---|---|
| query state | idle / loading / ready / failed |
| session materialization | 直接消费 VM 的 materialized / unmaterialized / unknown |
| history state | 直接消费 VM 的 pending / empty / non-empty / failed |
| Runtime lifecycle | running / paused / cancelled / failed / completed 等 |

`ready` 只表示详情查询已成功；不要求存在 durable reference 或至少一条历史事件。

首个成功响应为 `historyState=empty` 时进入 `ready-empty`，立即展示由 lifecycle、turnError 和 recoveryAction 决定的 UI。删除约 32 秒的“等到消息出现”轮询及其通用 missing-session fallback。

`total > 0` 但当前页 events 为空时继续执行首批分页；这与真实空历史不同。分页、generation / revision 和迟到响应保护继续遵守现有历史加载规则。

### 12.3 Shell 与交互优先级

1. 结构化失败或取消终态优先于旧 Starting / loading 投影，不能被迟到查询回退。
2. Direct + Open + 可继续：显示可输入的安全未物化状态。
3. Direct + Sealed/Unknown + 无 durable reference：显示具体原因和“重新发起会话”，不显示会再次尝试旧 Attempt 的普通发送暗示。
4. Workflow/AUTO 初始化失败或中断：保留专用 shell，隐藏普通 composer。
5. 已有历史：先完成必要分页，再展示 timeline；不能因为详情 VM 已 ready 就把未加载历史显示为空。
6. 错误区域只展示帮助用户决策和操作的文案，不暴露 fence、CAS、Raw 等实现术语。
7. 新按钮优先使用现有 shadcn/ui / prompt-kit copy-in 组件，不新增自研基础控件。

## 13. 显式恢复与结构化错误

### 13.1 “重新发起会话”

对于 Sealed / Unknown 且无 durable reference 的普通 Direct Attempt，提供桌面可视化按钮“重新发起会话”。

行为契约：

1. 复用现有重新执行 / 新建会话领域命令创建新的 canonical Attempt；UI 不直接复制目录或改 snapshot。
2. 新 Attempt 使用冻结或明确选择的 Task 配置，初始化自己的 Open fence；旧 Attempt 不重置、不删除、不伪装成已恢复。
3. 保留旧 Attempt 的 snapshot、timeline、Raw、附件、错误、queue 和 diagnostic，便于审计。
4. 当前未提交草稿按现有草稿归属保留；不把它暗中拼入新 Attempt 的首轮，也不自动重放失败消息。
5. 动作完成后明确导航到新 Attempt；用户可以在新会话中正常发送。
6. 复用现有进行中状态和目标级防重入，重复点击最多创建一个新 Attempt；不为此增加持久队列。
7. 只有既有领域命令会影响其他活跃执行时，才沿用现有冲突确认；不额外保留 command bar / terminal 入口。

### 13.2 结构化错误

实施时先检索现有错误目录并复用等价 code；缺失时增加有类型变体，至少覆盖：

| 建议 code | 含义 |
|---|---|
| `acp.session-creation-uncertain` | fence 已封闭但没有可用 identity |
| `acp.session-create-fence-unknown` | fence 缺失、损坏或版本未知 |
| `acp.session-identity-conflict` | 多个 identity 来源不一致 |
| `acp.session-creation-evidence-conflict` | fence 为 Open，但已有 Provider session 创建或活动的正向证据 |
| `acp.session-identity-persist-failed` | Provider 已返回 identity，但本地提交失败 |
| `acp.session-restore-reference-invalid` | 引用存在但类型、归属或配置不合法 |
| `acp.snapshot-schema-unsupported` | snapshot schema version 高于当前实现，读取只投影安全诊断且拒绝写入 |
| `acp.snapshot-invalid` | snapshot envelope 无法安全解析或 canonical Attempt 无法识别 |

错误只包含稳定 `code`、结构化 `params`、`recovery` 与内部 `diagnostic`。后端不包含对客文案；前端在 `web/src/locales/` 按 code 本地化并同步七语言。绝对路径、协议全文和内部堆栈不进入用户主提示。

## 14. 先红后绿验证方案

### 14.1 当前缺陷的最小失败测试与阶段归属

修改各阶段实现前必须先固定对应红灯：

1. 第二阶段：普通 Direct initialize 早停后，下一次发送被命令层错误选择为 Continue。
2. 第二阶段：RawAgent 使用 New + UserMessage 时，adapter 实际收到的正文丢失当前用户输入。
3. 第一阶段：unavailable + 有效 turnError + 无 sessionId + 空 timeline 被 VM 构造返回 `None`。
4. 第一阶段：详情成功且真实空历史时，前端仍因没有事件进入有界重试。

测试必须断言当前实际错误结果；不能先加入新字段，再把 fixture 解析失败包装成“复现成功”。

### 14.2 存储与目标解析契约

- 只有新普通 Direct Attempt 初始化 Open。
- legacy v0 缺 schemaVersion 时仍能读取已知 lifecycle / turnError，但 fence 为 Unknown；第一次合法 mutation 才升级为 v1。
- 字段缺失、null、未知 variant 和损坏数据均解析为 Unknown，不被写回 Open。
- fence 局部损坏不会导致整个 snapshot typed parse 失败；未知高版本与损坏 envelope 的写入被拒绝。
- seal 失败时 fake adapter 的 `session/new` 调用次数为 0。
- 同一 Attempt 的 Open 只能成功 seal 一次，operationId 和 revision 正确。
- Sealed / Unknown + no durable reference 永不自动 New，重启、队列重投和普通 retry 下同样成立。
- Open + no durable reference 但存在 Provider 派生正向证据时只 Blocked，不自动 New；只有本地 user timeline item 时不误判为已物化。
- 有合法 durable reference 时始终 Continue；worker-ref 缺失可重建，connection generation 变化后重新 attach。
- identity 别名空值、别名冲突、snapshot / worker-ref 冲突和 Provider 归属不匹配分别固定。
- lifecycle、title、config、terminal、repair 等事务不能清除 fence、identity 或较新的 turnError。
- mutation 保留未知顶层字段；旧完整对象 writer 不能清除 v1 字段。
- Dynamic、Workflow、AUTO、manual check、非 current 和只读路径不能取得 Direct create permit。

### 14.3 确定性竞态与故障注入

使用 fake adapter、可注入存储失败和 barrier / channel 控制顺序，不使用固定 sleep：

| 用例 | 必须断言 |
|---|---|
| 两个请求同时进入同一 Attempt | 只有一次 seal 成功，`session/new <= 1` |
| 锁外观察无 durable reference，进入串行通道时已有 durable reference | 最终 Continue，使用 DirectFollowUp |
| Stop 先于 seal 事务 | `session/new = 0`，fence 保持 Open |
| seal 先于 Stop | `session/new <= 1`，停止后不发送旧 turn 的 prompt |
| Stop 后 identity 迟到 | 旧 lifecycle owner 已失效时仍可按相同 fence createOperationId 提交 durable reference；lifecycle 不回退，下一轮 Continue |
| seal 后、真正发送前退出 | 重启后 Sealed，不自动 New |
| 请求发送后 Raw 写入失败 | Sealed 保留，不根据 Raw 缺失再次创建 |
| Provider 响应丢失 | Sealed + no durable reference，展示明确恢复动作 |
| durable reference 提交失败 | 不发送 `session/prompt`，错误可查询 |
| durable reference 成功、worker-ref 失败 | 下次修复引用并 Continue |
| 旧 snapshot writer 晚到 | CAS / 合并阻止其覆盖新 durable reference、Stop 或错误 |

进程退出窗口优先用子进程夹具验证原子提交后的磁盘事实；测试外部命令必须复用项目隐藏窗口创建方式，并在结束后清理。

### 14.4 Prompt 请求体验收

必须检查 fake adapter 收到的最终请求体：

- DirectInitial 严格保持空 system prompt，只包含冻结的 Direct 用户原始输入、引用和附件，不含 Runtime hidden/profile/goal/output/repair 内容。
- DirectFollowUp 只包含本轮 durable `promptSubmission`，不重复原始输入。
- DirectRecoveryBootstrap 同时包含冻结的原始 Direct 用户输入和当前 durable `promptSubmission`，当前指令优先，且不引入 Direct 禁止的隐藏上下文。
- 锁外闭包、IPC 参数或最新 UI 草稿与 durable submission 不一致时，实际请求必须使用 durable submission。
- UI timeline 中原始与当前两条用户输入各出现一次；bootstrap 的包装标签不进入气泡，重复恢复不产生重复事件。
- Direct 契约允许的附件、引用、workspace 文件、模型、权限和 config options 不丢失、不重复；profile/role hidden content 不得借恢复路径重新注入。
- 目标在等待期间由 Create 变为 Continue 时，实际使用 DirectFollowUp，不发送提前生成的 bootstrap。
- 原始输入缺失时明确失败，不发送空替代 Prompt。
- attachment-only 合法发送仍然成立。

### 14.5 VM、前端与 DOM 测试

- snapshot 存在、无 sessionId、空 timeline 时仍返回 Attempt VM。
- 有效 turnError 被保留；fence 局部损坏、null 或畸形错误不会制造假失败或吞掉其他可读字段。
- VM 明确返回 snapshotRevision、materialization、historyState 与 recoveryAction，前端不从 null / 空数组组合猜测。
- `historyState=empty` 的成功详情进入 ready-empty，不启动约 32 秒重试。
- `historyState=pending` 时继续首批分页，不误判为空会话。
- Sealed / Unknown 展示具体错误和“重新发起会话”。
- Workflow/AUTO 初始化失败与中断仍显示专用 shell，DOM 中没有普通 composer。
- Dynamic 严格缺引用不 New。
- 新 turn / 新 revision 不被旧 scope、旧 generation 或迟到 VM 覆盖。
- “重新发起会话”重复点击只创建一个 Attempt，导航正确，旧数据和草稿策略正确。
- composer 输入、错误展开和空态变化不引起无关历史树全量重渲染。

### 14.6 构建与客户端验收

1. 运行受影响的 ACP、Runtime、desktop command 和 fake adapter 定向 Rust 测试。
2. 运行 `acp-chat-events`、`acp-session-shell`、新增 DOM / API 契约测试和 TypeScript 检查。
3. 完成生产前端构建和项目要求的 Rust 检查。
4. UI 验证优先使用当前 agent 内置 iab，无法完成时使用已连接 Chrome，最后才使用 agent-browser；deep link 到目标会话。
5. 验证 Direct initialize 早停后原地恢复、创建结果不确定、具体失败投影、显式新 Attempt、第二条 Continue 和应用重启。
6. 回归 Workflow/AUTO 初始化失败 / 中断、正常历史和 Dynamic 严格恢复。
7. 涉及真实 Stop、退出和 Windows 客户端生命周期时补 EXE 验证，不能只用浏览器 mock 代替。
8. 清理浏览器页面、fake adapter、测试进程、临时 workspace 和临时文件。

## 15. 预计代码与文档影响范围

### 15.1 预计代码修改

第一阶段只涉及 `src-tauri/src/view_models.rs`、`web/src/components/acp/ACPChatDialog.tsx`、必要的 session shell/readiness helper 及对应 Rust/Web 测试；下表其余模块属于第二、三阶段，不得提前修改。

| 模块 | 职责 |
|---|---|
| `src/acp/events.rs` 或现有 snapshot 领域模块 | raw envelope、schemaVersion、fence 字段级解析、现有事务扩展、durable reference 提交与未知字段保留 |
| `src/acp/session_target.rs`（建议） | durable reference / attached identity 归一化、deny-only evidence guard、Direct 资格与最终目标解析 |
| `src/acp/client.rs` | 串行通道内解析、seal permit、创建顺序、迟到 identity |
| `src/acp/connection.rs` | 禁止 `session/new` 透明重试；按需要提供发送边界测试注入 |
| 普通 Direct Attempt 创建写入点 | 新 Attempt 初始化 Open |
| `src/app/mod.rs`、`node_executor.rs` | 只携带 turn identity 的 intent、durable submission 重读、冻结 Direct 输入和按目标准备 Prompt |
| `src/provider/mod.rs` | 传输目标与内容语义解耦、DirectInitial / DirectFollowUp / DirectRecoveryBootstrap 构造 |
| `src-tauri/src/commands.rs` | 移除“Attempt 存在即 Continue”，只提交意图 |
| `src-tauri/src/view_models.rs` | Attempt 级失败投影、snapshotRevision、materialization、historyState 与 recoveryAction |
| 现有重新执行 / 新会话命令 | 显式新 Attempt 和防重复提交 |
| `web/src/components/acp/ACPChatDialog.tsx`、`web/src/lib/acp-session-shell.ts` | 查询、物化、历史和 lifecycle 状态拆分 |
| `src/prompts/<language>/runtime/`、`web/src/locales/` | DirectRecoveryBootstrap 与错误 / 动作文案 |
| Rust / Web 测试 | 红绿证据、事务、竞态、请求体、VM 和 DOM 契约 |

模块清单表示影响面，不要求机械修改每个文件；具体落点以实施时现有职责为准。不得为了少改文件把 ACP 领域逻辑塞进桌面命令或 React 组件。

### 15.2 实施时必须同步的文档

1. `产品设计文档/interaction/app/conversational-runtime.md`：把“Direct 未物化时直接重新 session/new”改为 fence 决策；区分 durable reference 与 attached identity；本地 user timeline item 不再单独构成 `sessionEstablished`；补 ready-empty 与显式恢复行为。
2. `产品设计文档/runtime/control.md`：create fence、session identity、Stop 与迟到提交。
3. `开发计划/acp接入/acp-ui.md`：Attempt VM、ready-empty、分页和 shell 优先级。
4. `开发计划/direct模式/Direct模式完整设计与开发方案.md`：把“复用原 ACP session”拆分为已物化 Continue 与未物化 fence 决策；明确 DirectRecoveryBootstrap 是空 system prompt 契约下仅组合两次用户输入的恢复例外，不允许 Runtime hidden/profile/goal 内容。
5. `开发计划/生命周期整理/工作流停止后Runtime控制与自由会话统一技术方案.md`：共享事务与严格模式隔离。
6. `开发计划/gold-band-mvp-plan.md`：实施、测试、客户端验收和发布记录。
7. 已有 ACP snapshot schema、结构化错误目录或存储说明真源：登记 snapshot `schemaVersion=1`、legacy v0、高版本拒写和字段级 Unknown 读取规则。

第一阶段已同步第 1、3、6 项中与失败投影、ready-empty、分页和 shell 边界相关的内容；其余 create fence、identity、Prompt、schema 与恢复动作相关文档继续在第二、三阶段实施并验证时同步。

## 16. 实施顺序

### 16.1 第一阶段：失败投影与空历史加载

1. 建立第 14.1 节第 3、4 项最小失败测试，保存红灯证据。
2. 修改 Attempt VM：保留纯 runtime-control placeholder 的 `None` 语义，但 snapshot 已包含非 `none` lifecycle 或合法 `turnError` 时必须返回 VM。
3. 修改前端 readiness：active 且尚未形成 canonical 内容的初始化继续等待；非 active 且 `eventPage.total = 0` 的成功响应立即进入 `ready-empty`；`total > 0` 且当前页为空继续分页。
4. 回归 Direct、Workflow/AUTO、Dynamic shell、迟到 query/live generation 防护和现有分页窗口语义。
5. 运行定向 Rust / Web 测试、TypeScript、生产构建和内置浏览器 deep-link 验收，记录结果并同步相关产品设计与开发计划。

实施结果（2026-10-26）：

- [x] 后端只过滤 unavailable、无 sessionId、无可展示 timeline、无 Agent branch、且没有非 `none` lifecycle / 合法 `turnError` 的纯 runtime-control placeholder；未物化失败 Attempt 返回 VM 并保留结构化错误。
- [x] 前端拆分“详情查询 ready”与“canonical 内容已物化”语义；settled 且 `eventPage.total = 0` 的空响应进入 `ready-empty`，active 空响应（包括携带旧 `turnError`）继续等待，`eventPage.total > 0` 的空当前页继续分页。
- [x] Workflow/AUTO 初始化中断/失败只消费 canonical 内容 helper，不被空失败 VM 覆盖；现有 Dynamic、迟到响应、分页窗口与重进收敛测试保持通过。
- [x] `acp.session-restore-reference-missing` 七语言文案移至与错误码一致的 `errors.acp.*`，简体中文明确说明 Provider 会话引用缺失、原会话无法恢复以及新建会话后的恢复动作；错误横幅标题与原因分层展示并使用 alert 语义，不再只显示英文 diagnostic。
- [x] 提示修正回归：最小映射测试原实现只返回英文 diagnostic；修复后相关 5 个 Web 测试文件 265/265、生产构建与 diff 校验通过。内置 iab 在 787px 视口验证标题/原因分层且无横向溢出；精确错误码文案由 DOM 用例验收。
- [x] 红灯：Rust 最小用例在原实现上得到 `None`；Web idle/terminal 空响应仍不 ready；补充的 active + stale `turnError` 用例在原实现上错误返回 ready。
- [x] 绿灯：Rust `acp_session_vm_` 过滤用例 5/5；Web 受影响 5 个文件 300/300，其中 DOM 用例固定未物化失败首个响应显示错误且 180ms 后仍只有 1 次查询，纯 placeholder 仍按既有路径重试。
- [x] 静态与构建：`cargo fmt --all -- --check`、`cargo check -p gold-band-desktop --tests`、`npm run web:build`、`git diff --check` 通过。Rust 仅有 2 条既有 dead-code warning；Vite 仅有既有 dynamic import / 大 chunk warning。
- [x] 浏览器：内置 iab deep link 验证 Direct settled 正文、active“回复生成中”、Workflow 暂停专用继续入口，以及 720px 窄窗口无横向溢出、恢复宽度后状态不丢失。浏览器 preview 没有“未物化 + 空 timeline 失败”fixture，该精确路径由 DOM 测试验收；未打包或验证真实 EXE 现场。
- [x] 测试浏览器、Vite 开发服务和临时资源已清理。
- [x] 完成后自评审：复用既有 snapshot lifecycle、Attempt VM、`eventPage.total`、readiness 重试和 shell 判定，不新增 DTO、持久字段、状态机、缓存、队列或依赖；每次查询只增加常数级字段判断，失败场景减少重复请求，timeline I/O、窗口上限与 React 订阅范围不变，无需专项 benchmark。

### 16.2 第二阶段：Direct 安全恢复闭环

1. 建立第 14.1 节第 1、2 项红灯。
2. 实现 create fence、durable reference 与现有 metadata transaction 的必要写入保护及崩溃测试。
3. 把 Direct intent 的最终目标解析移入 Attempt Prompt 串行通道，实现 deny-only evidence guard、seal permit 和 durable reference 立即提交。
4. 解耦 Provider 传输与 Prompt 内容，实现从冻结 Direct 输入和 durable submission 构造的 DirectRecoveryBootstrap，并完成请求体与双用户事件测试。
5. 接通结构化恢复错误和必要的桌面恢复动作，完成真实 Stop、重启和 EXE 验收。

### 16.3 第三阶段：基础设施治理

1. 根据第二阶段暴露的真实写入边界决定是否引入 snapshot schemaVersion 和更完整的 typed mutation；没有明确不变量需求时不扩建。
2. 收口剩余旧完整对象 writer、完成 identity 类型传播和跨版本拒写测试。
3. 同步第 15.2 节其余产品设计与开发计划，记录真实红绿证据后再标记整体完成。

顺序用于降低风险，不代表可以只上线错误提示或只放宽 New 分支。create fence、identity、Prompt 内容、失败投影和显式恢复构成一个完整正确性闭环。

## 17. 过度设计与性能自评审

### 17.1 过度设计评审

结论：新增一个单调 create fence、snapshot envelope version 和两个不同生命周期的 identity 类型，是解决外部创建副作用不确定性与连接代隔离的最小必要设计；snapshot 写入只扩展现有 metadata transaction，不新增第二套事务或 lifecycle。

- 现有 sessionId、revision 和 lifecycle 无法表达“尚未授权创建”与“已经可能发出创建”之间的差异，因此 fence 有明确不变量依据。
- fence 不复制 sessionId、availability 或 lifecycle，只表达一次性发送权。
- 复用既有 Attempt Prompt 串行通道、metadata file lock、owner / revision、原子文件写入、Prompt Queue 和重新执行入口。
- `ValidatedDurableProviderSessionReference` 与 `AttachedProviderSessionIdentity` 分别封装跨进程恢复和当前连接代 attachment 的现有真实不变量，不新增 aggregate 或复制 session 数据。
- snapshot schemaVersion 只服务该单文件的安全演进；不建设通用迁移平台，也不与 Runtime node 的 `acp_storage_schema_version` 合并成新的全局版本系统。
- DirectInitial、DirectFollowUp、DirectRecoveryBootstrap 和 typed permit 只封装真实接口不变量，不构建通用工作流框架。
- 不新增数据库、后台 repair 服务、全局缓存、锁表、队列或 Provider 专用适配层。
- 当前依赖已经能完成 serde、原子写入、结构化错误和 UI 控件，不需要引入新的开源库；实施前仍应核实既有 atomic-write-file 在 Windows 目标平台的替换与 flush 保证。

### 17.2 性能影响评审

- 数据规模：每次只读取和写入当前一个 Attempt 的固定小 snapshot；DirectRecoveryBootstrap 只读取该 Attempt 冻结输入和当前 durable submission，不随 workspace 总历史增长。
- 目标解析和 identity 归一化为 O(1)，不扫描 Raw、全部 timeline、usage 或所有 Task。
- 首次创建增加两次必要可靠提交：seal 和 identity；正常 Continue 不写 fence，也不构建 bootstrap。
- 网络 initialize、`session/new`、附件处理和模型响应不持有 snapshot 文件锁。
- snapshot、worker-ref、timeline index 和相关文件读取继续运行在 blocking worker pool，不占用 Tauri async/UI 调度线程。
- deny-only evidence guard 只读取当前 snapshot、worker-ref 与有界 timeline/index 摘要，不扫描完整 Raw、timeline、usage 或全部 Task。
- 不引入 N+1 请求、全量启动迁移、无界缓存、无界队列或重复诊断扫描。
- 前端删除约 32 秒轮询，减少失败场景请求量；ready-empty 不增加全局订阅范围。
- timeline 仍按现有分页加载；Attempt VM 只增加少量状态字段，不把历史正文嵌入摘要接口。
- 实施验收应记录首次创建新增写入次数、正常 Continue 路径是否保持原有 I/O 量、失败查询请求次数和 React 渲染范围。
- 当前没有证据表明需要 benchmark 或新缓存；若实测原子提交或 Prompt 准备明显退化，再定义数据规模、基线与目标后专项优化。

## 18. 发布、旧数据与回退

- snapshot `schemaVersion=1` 是向前演进边界。旧数据不全量迁移，缺版本按 legacy v0、缺 fence 按 Unknown 保守读取；只有发生合法 mutation 时才原位升级并保留未知字段。
- 未知高版本允许只读投影安全诊断，但拒绝 mutation；不得由旧实现整份覆盖。
- 不根据 Raw、timeline、文件时间戳或诊断文本补造 Open。
- 能结合 canonical 配置构造可信 durable reference 的旧数据继续 Continue，并可在写路径补齐 snapshot / worker-ref；旧 attached identity 不跨 connection generation 复用。
- 无 durable reference 的旧数据通过 Attempt VM 显示具体状态和“重新发起会话”。
- 不删除或重写旧 snapshot、worker-ref、timeline、Raw、附件和错误来伪装修复成功。
- 不允许新旧版本同时写同一 workspace。旧版本可能不保留新字段；回退后再升级时字段缺失仍必须是 Unknown。
- 回退不能通过删除 fence、清空 durable reference 或重开旧 Attempt 恢复旧行为。
- Provider 已创建但 durable reference 永远丢失的远端空会话保持外部未知结果；除非 Provider 将来提供可验证查询 / 幂等能力，否则客户端不按名称猜测、接管或清理。
- 发布前保留测试现场和必要备份，验证旧数据读取、正常启动、崩溃重启和跨版本拒绝写入行为。

## 19. 完整验收清单

- [ ] 当前四个缺陷均有修改前红灯和修改后绿灯证据。
- [ ] 新普通 Direct Attempt 在任何 Provider 副作用前可靠初始化 Open。
- [ ] snapshot schemaVersion=1、legacy v0 读取、字段级 Unknown 和未知高版本拒写均有契约测试。
- [ ] 缺失、null、损坏和未知 fence 均为 Unknown，不被默认或 repair 为 Open。
- [ ] Open 只能原子转换一次 Sealed；seal 失败时 `session/new = 0`。
- [ ] typed permit 绑定 Attempt、operationId、配置和 revision，`session/new` 无透明重试。
- [ ] Sealed / Unknown + no durable reference 在重启、重投和并发下都不会自动 New。
- [ ] Open + no durable reference 但存在 Provider 派生正向证据时只拒绝创建；本地 user event 不误判为 session 已物化。
- [ ] durable reference 与 attached identity 分层正确；connection generation 变化时重新 attach，worker-ref 缺失可修复，冲突明确失败。
- [ ] durable reference 在 Provider 返回后立即提交；Stop 后旧 owner 失效时仍按 fence createOperationId 保存，且不复活 lifecycle。
- [ ] snapshot 所有相关写路径使用统一事务，不再由旧对象整份覆盖新事实。
- [ ] 最终目标在 Attempt Prompt 串行通道内解析，命令层不按 Attempt 存在推断 Continue。
- [ ] DirectInitial、DirectFollowUp、DirectRecoveryBootstrap 与 New / Continue 正交。
- [ ] DirectRecoveryBootstrap 只包含冻结的原始 Direct 用户输入和当前 durable promptSubmission，system prompt 为空且不含 Runtime hidden/profile/goal/output/repair 内容。
- [ ] UI timeline 中原始与当前用户输入各显示一次，bootstrap 包装标签不进入气泡，重试不产生重复事件。
- [ ] 有效 turnError 在无 sessionId、空 timeline 时仍进入 Attempt VM。
- [ ] Attempt VM 明确包含 snapshotRevision、materialization、historyState 和 recoveryAction。
- [ ] `historyState=empty` 进入 ready-empty，不再执行约 32 秒内容驱动轮询。
- [ ] 有历史未加载时继续分页，不误判真实空历史。
- [ ] Workflow/AUTO 初始化 shell 和 Dynamic 严格恢复均回归通过。
- [ ] Sealed / Unknown 提供桌面“重新发起会话”，创建新 Attempt，不重置旧 fence。
- [ ] 新 Attempt 防重复创建，旧数据、草稿和 queue 按契约保留且不暗中重放。
- [ ] 结构化错误只有 code / params / recovery / diagnostic，前端七语言本地化。
- [ ] 新增或修改 Prompt 位于 `src/prompts/`，遵守 prompt authoring 和七语言规则。
- [ ] 定向 Rust / Web 测试、类型检查、生产构建和故障注入通过。
- [ ] 内置浏览器和必要 EXE 客户端验收完成，测试资源已清理。
- [ ] 产品设计文档、关联开发计划、schema 与错误目录在实现完成时同步。
- [ ] 完成后重新进行过度设计与性能影响评审，并记录真实数据。
