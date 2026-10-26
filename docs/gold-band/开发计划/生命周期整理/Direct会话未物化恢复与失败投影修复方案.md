# Direct 会话未物化恢复与失败投影完整修复方案

- 状态：设计修订完成，待实施、待红绿测试与客户端验收
- 日期：2026-10-22
- 适用版本：0.20.0 后续修复版本，实际发布版本在验收后确定
- 方案定位：修复会话建立事实、发送内容、并发与持久化、失败展示四个边界；以完整正确性为目标，不再受“只修改四个文件、不能增加持久字段”的限制。
- 本次交付范围：只更新本方案文档，不代表实现已经完成。
- 关联文档：
  - `docs/gold-band/开发计划/生命周期整理/Direct会话未物化恢复与失败投影修复方案-影响评估.md`
  - `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
  - `docs/gold-band/产品设计文档/runtime/control.md`
  - `docs/gold-band/开发计划/acp接入/acp-ui.md`
  - `docs/gold-band/开发计划/direct模式/Direct模式完整设计与开发方案.md`
  - `docs/gold-band/开发计划/生命周期整理/工作流停止后Runtime控制与自由会话统一技术方案.md`

## 1. 结论与设计取舍

本方案保留 Direct 初始化期间的停止能力，完整支持可证明未开始创建 Provider session 的当前普通 Attempt 在下一次用户发送时原地恢复；同时修复结构化错误丢失与无效加载。

采用以下设计：

1. 在现有 ACP snapshot 内增加一项有类型的“首次 session/new 请求授权事实”，与现有 lifecycle、session identity 由 ACP 领域统一管理。
2. 会话目标由 ACP 领域解析；桌面命令层只提交调用意图和已校验的业务上下文，不解析 Raw JSONL、不推断 Provider 身份。
3. 最终目标选择、请求授权与身份提交进入既有 attempt prompt 串行执行通道，复用现有 owner / revision CAS。
4. 会话传输方式与 Prompt 内容语义分开。Direct 首次恢复明确组合原始需求上下文和本轮消息，实际发送正文、附件、引用和隐藏上下文都必须验收。
5. 所有模式保留有效结构化失败详情；前端区分“详情已加载”和“会话已建立”，继续保留 Workflow/AUTO 初始化失败、中断页面。
6. 已发出或可能发出 session/new、但没有可用 sessionId 的场景，不自动再次 New；历史缺少新事实的场景也不靠日志补造确定性，提供显式重新发起入口。

必须说明的能力边界：

- 本地不能仅凭没有 sessionId、没有 Raw 记录，证明远端从未建立过会话。
- 未获 Provider 明确幂等或查询能力保证时，客户端无法同时承诺“创建响应丢失后自动恢复”和“远端恰好创建一次”。
- 本方案保证：对于受保护的 Direct 首次创建，结果不确定后不自动重发 session/new；不会静默丢弃已知历史，也不会把未知状态当成未创建。
- 历史现场可立即得到具体失败原因与可执行恢复入口，但不承诺所有旧记录都能无确认地原地恢复。

这些边界是最终产品契约，不在实现时回退成“缺引用就 New”或“扫描日志没有找到就 New”。

## 2. 背景、证据与根因

### 2.1 现场链路

用户停止 Direct 首次执行，重新打开历史会话并继续发送，最终看到：

> ACP 会话失败，无法获取当前会话，未获取到具体原因。

现场材料与当前代码共同支持以下链路：

1. 首次执行在 initialize 阶段停止，Run / Node 进入可继续暂停；本地没有有效 worker-ref、sessionId 和 timeline。
2. 桌面 `existing_attempt_prompt_session_target` 将既有 Attempt 一律解释为 Continue。
3. ACP 公共入口返回 `acp.session-restore-reference-missing`。
4. durable turnError 已落盘，但后端将 unavailable、无 sessionId、无可展示 timeline 的快照提前返回 None。
5. 前端没有接受该失败详情，经过约 32 秒有界重试后显示通用缺会话原因。

“现场 Raw 未见 session/new”只能解释现场现象，不能升级为对所有会话的完整性证明。

### 2.2 已核实的代码证据

以下行号是 2026-10-22 工作区定位参考，实施时按符号重新定位。

| 证据 | 当前实现位置 | 含义 |
|---|---|---|
| 既有 Attempt 一律 Continue | `src-tauri/src/commands.rs:141` | Attempt 存在被错误等价为 Provider session 可恢复 |
| 普通追问固定 Continue + UserMessage | `src/app/mod.rs:5783` | 只增加 SessionMode 参数不足以完成恢复 |
| RawAgent New 读取 requirement，UserMessage 的 requirement 为空 | `src/provider/mod.rs:2380` | 直接 New + UserMessage 会丢失本轮实际发送正文 |
| 准备接口清空首轮隐藏上下文 | `src/app/mod.rs:5828` | 恢复时不能沿用普通追问的上下文清理策略 |
| prompt lock 在 run_prompt_inner 内获取 | `src/acp/client.rs:3094` | 锁外预判与复核不能形成原子授权 |
| 先创建传输请求，后记录 outbound Raw | `src/acp/client.rs:5836` | 请求可能已经发送，但日志尚未记录 |
| Raw 写入失败仍继续执行 | `src/acp/client.rs:1623` | 完整读到 Raw EOF 也不等于协议记录完整 |
| worker-ref 在 session setup 成功后写入 | `src/acp/client.rs:3249` | 远端成功到本地引用持久化之间有故障窗口 |
| unavailable 失败快照被提前丢弃 | `src-tauri/src/view_models.rs:4150` | 具体失败原因无法进入 UI |
| 初始化 shell 使用 !baseSessionReady | `web/src/lib/acp-session-shell.ts:147` | 全局放宽 ready 会改变 Workflow/AUTO 页面 |
| readiness 要求终态已有事件 | `web/src/components/acp/ACPChatDialog.tsx:12390` | 需要保留“历史尚未加载”的保护 |
| 非 Completed Run 不允许 outcome | `src/runtime/mod.rs:431` | 现有校验已覆盖，不能误称完全缺少 Run.outcome 校验 |

### 2.3 根因归类

- **局部设计缺陷**：缺少可靠的“创建请求是否可能发出”事实；以 Attempt 存在决定 Continue；RawAgent 使用 SessionMode 隐式决定消息正文。
- **实现不完整**：终态错误详情被丢弃，详情加载完成与会话已建立混用，派生 worker-ref 与身份事实的写入顺序不完整。
- **分布式故障边界**：创建请求可能已成功但响应未可靠落盘。这不能用 UI 状态、固定延时或再次扫描诊断日志消除。

## 3. 范围与用户可见行为

### 3.1 行为矩阵

| 场景 | 后端行为 | 用户可见结果 |
|---|---|---|
| 新格式 Direct 在 initialize 前或期间停止，创建授权尚未提交 | 下一次发送可在原 Attempt 创建 session | 保留停止能力，发送后正常进入新轮次 |
| snapshot 或 worker-ref 提供一致、有效的既有身份 | Continue；必要时在写路径修复派生引用 | 继续原会话，不重复创建 |
| session/new 已获授权，结果未知且无身份 | 结构化失败，禁止自动 New | 显示具体原因，提供“重新发起会话” |
| 历史记录缺少新事实且无身份 | Unknown，禁止自动 New | 显示已知错误；通过显式入口重新发起 |
| 已知身份冲突、配置归属冲突或记录损坏 | 结构化失败，保留原记录 | 显示对应错误，不静默选择某份身份 |
| Direct 无 session、有效 turnError、真实空历史 | 返回失败详情，首个有效响应后结束加载 | 可见具体错误与允许的恢复操作 |
| Workflow/AUTO 无 session、无历史的初始化失败或中断 | 返回可消费详情，保留初始化 shell 语义 | 显示失败或中断页面，隐藏普通聊天 composer |
| 有历史但本页未加载 | 正常分页获取首批事件 | 不把“没加载”显示成空聊天 |
| Dynamic / Workflow / AUTO 缺恢复引用 | 保持严格恢复 | 不因本次 Direct 修复自动 New |

### 3.2 不扩大到的事项

不引入新的会话 aggregate、业务 identity、通用重试平台、消息队列、缓存或 Provider 专用名称判断；不修改 Workflow/AUTO 的业务调度状态机，不给 Dynamic inner Attempt 开放 Direct 自动重建权限。

共享 ACP 的持久化与错误投影需要同步完善，但 Direct 的首次创建限制不能直接套用到其他模式已经明确授权的 New / 自动重试策略。

## 4. 数据归属与权威事实

### 4.1 现有事实继续负责原有职责

| 数据 | 所属领域与职责 |
|---|---|
| Task authoring / Run snapshot / requirement / inputs | 原始需求、Agent 配置和输入来源；恢复不得读取别的 Task 或漂移到最新模板 |
| Run / Round / Node | canonical current locator、Runtime 状态、控制权与继续资格 |
| ACP snapshot | ACP lifecycle、Provider session identity、创建请求授权事实与 turnError |
| worker-ref | 面向 Provider 的可恢复引用投影；不能成为独立生命周期 |
| timeline / prompt usage | 用户消息、Agent 输出和统计事实；不反推远端不存在 |
| Raw / diagnostics | 审计与定位；不作为自动 New 的否定性证据 |
| Prompt Queue | 待发送输入与顺序；不保存另一套会话目标判断 |

### 4.2 新增的最小 durable 事实

在现有 `acp.snapshot.json` 增加可选、有类型的 `providerSessionCreation`：

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum AcpProviderSessionCreation {
    NotRequested,
    Requested {
        #[serde(rename = "operationId")]
        operation_id: String,
    },
}
```

外层字段通过现有 metadata 的 camelCase 序列化规则映射为 `providerSessionCreation`；未知与缺失值必须按下述显式规则解析，不能使用 enum 默认值补齐。

语义：

- 字段存在且为 NotRequested：新版本在真正的新 Attempt 创建路径中可靠初始化，尚未向该 Direct Attempt 授予首次 session/new 的发送权。
- Requested：本地已为某个既有 operationId 提交发送授权；远端可能尚未收到、正在创建、已经创建或响应丢失。
- 字段缺失、null、未知变体或损坏：Unknown，绝不默认 NotRequested。
- 已建立身份继续使用现有 `sessionId`；不在新字段复制 sessionId、availability、turn status 或另一套 revision。
- operationId 复用既有执行身份，仅用于绑定创建授权与响应，不新增 Provider identity。

新增事实的必要性：当前 unavailable + no sessionId 既可能是“还没发请求”，也可能是“请求已成功但本地丢失响应”；现有 revision 只能区分本地写入代际，不能区分这两种外部副作用状态。新字段补齐这一不变量，不复制 lifecycle。

### 4.3 初始化与单调性

1. 仅在真正分配新普通 Direct Attempt 的写入路径初始化 NotRequested，且必须先于任何 Provider 连接或请求副作用。
2. 不能在 `begin_session_turn`、历史读取、缺文件补建、发送重试或进程重启时，因为“当前没文件”就补成 NotRequested。
3. 新 Attempt 的 node 与 snapshot 采用现有创建边界和原子文件提交。中途失败且 Provider 尚未启动时允许保守停在 Unknown；不得对外发布可执行成功。
4. 对受保护 Direct，NotRequested -> Requested 单调前进；取消、失败、重启、新 turn、队列重投都不能重置为 NotRequested。
5. generic metadata rewrite、title/config 刷新、terminal settlement 与 legacy normalization 必须保留该事实以及已提交的身份；缺字段的旧内存对象不能清除它。
6. 未知或更高格式不能静默当旧默认值读取；写路径返回结构化错误，查询保留可展示的其他已知字段。
7. 不把该字段塞入 Node 的 timeline storage schema，也不为它增加全库启动扫描。

## 5. ACP 领域接口与身份归一化

### 5.1 解析与意图分开

在 `src/acp/` 下集中定义类型和解析，建议模块 `session_target.rs`；具体命名以现有模块边界为准。

对外表达两类输入：

- **严格目标**：Runtime 或 Dynamic 已明确决定 New / Continue 的调用，沿用既有模式规则。
- **Direct 恢复意图**：仅由经过 canonical 校验的普通 Direct 发送入口提交，允许在安全条件下从“继续”解析为首次创建。

领域解析结果表达：

```text
Continue(validated_restore_target)
CreateDirectSession
Blocked(structured_runtime_error)
```

解析结果是瞬时执行计划，不持久化。物化事实则从 snapshot、worker-ref 和新增创建授权事实读取，不能从桌面层传入一个未经复核的“未物化=true”。

### 5.2 身份归一化

统一归一化 `acpSessionId` 与现存 `sessionId` 别名，供发送入口、VM、摘要与恢复校验共用：

1. 对每个候选分别验证类型、trim 后非空，再比较；不能让空 acpSessionId 遮蔽非空 sessionId。
2. 同一对象两个非空别名不同，或 snapshot 与 worker-ref 身份不同，返回身份冲突，不猜测较新者。
3. Provider、adapter 和 workspace 归属必须匹配 canonical Attempt 与冻结配置。不能仅凭任意非空字符串认为可恢复。
4. Snapshot 有可信身份而 worker-ref 缺失时，可在用户发送的写路径重建 worker-ref，再严格 Continue。
5. worker-ref 有可信身份而旧 snapshot 缺身份时，可在写路径补齐 snapshot。查询只投影，不顺便修复。
6. Provider 工作目录从 canonical Run / worktree 配置解析；不能把旧 snapshot 中可能表示 attempt 目录的 cwd 直接当 Provider cwd。
7. 非法、矛盾或归属不明的引用不会因归一化而转为允许 New。
8. ACP 公共 Continue 校验与 setup_session 使用同一已校验类型，消除“一处接受 sessionId，另一处只接受 acpSessionId”的分叉。

新数据中 snapshot 的 session identity 是统一管理的 durable 事实，worker-ref 是可重建投影。读取旧引用只是既有事实归一化，不引入第二套新存储或双写生命周期。

## 6. 最终决策、持久化与副作用顺序

### 6.1 复用既有串行执行通道

最终解析必须位于 `run_prompt_inner` 获取既有 attempt prompt lock 之后，而不是调用 run_prompt 前重复检查两次。

- 复用当前完整 attempt 作用域的执行通道；不另建临时 Mutex 或第二张锁表。
- snapshot metadata lock 只覆盖小文件读取、owner / revision CAS 和原子提交；不在此锁内等待 initialize、session/new、网络响应或附件转换。
- 复用桌面 single-instance 与现有 workspace 写入约束；不把进程内 Mutex 宣称为跨机器或多进程锁。
- 同一 workspace 被不受支持的旧进程、外部工具或同步盘并发改写，不在自动恢复保证范围；若产品后续允许多写入进程，应先补 workspace 级排他写入契约。

### 6.2 接口调整

`client::run_prompt` 接收明确的 session intent；对 Direct 恢复，允许在最终目标确定后调用 Prompt preparation factory。

逻辑契约如下，签名细节在实施中按现有 Rust 类型整理：

```text
run_prompt(session_intent, existing_owner, prepare_prompt_for_resolved_target, ...)
    -> 获取既有 attempt prompt 执行通道
    -> 重新校验 owner / canonical 业务资格
    -> 解析最终 Continue / Create / Blocked
    -> 只准备对应目标需要的 Prompt
    -> 副作用前再次校验取消与创建授权
    -> 执行 Provider 交互并提交事实
```

普通 Continue 不提前构建 bootstrap Prompt。目标在锁内由 New 变为 Continue 时，必须使用 Continue 的正文与附件，不能仅改一个 SessionMode 却继续发送已拼好的 bootstrap 内容。

需要更新所有调用方与测试，不保留旧签名、隐式猜测目标的入口或兼容 wrapper。

### 6.3 首次创建的写入顺序

受保护 Direct 的正常链路：

```text
新 Attempt：可靠初始化 NotRequested
-> durable admission / claim
-> attempt prompt 串行执行通道
-> 完成 initialize，准备本次 Prompt
-> 短事务重新读取 owner、取消状态与创建事实
-> NotRequested -> Requested(operationId) 原子可靠提交
-> 提交成功后，最多调用一次 session/new
-> 得到合法 sessionId
-> 先单独可靠提交 Provider identity
-> 写入 / 修复 worker-ref 投影
-> 记录用户消息及已有 prompt identity
-> 校验本轮仍被允许执行
-> session/prompt
```

具体约束：

1. 创建授权提交失败，禁止调用 session/new。
2. 授权成功后即使确定尚未发送就发生异常，也保守保留 Requested；不以捕获到某类通用异常为由重置。
3. session/new 响应成功后，先提交轻量 identity，不等待 timeline checkpoint、标题刷新、usage 修复或完整 VM。
4. identity 提交失败时，不发送用户 Prompt；保留 Requested，返回结构化持久化错误，后续不得自动再 New。
5. identity 已提交而 worker-ref 写入失败时，后续发送修复投影并 Continue；不再创建 session。
6. 保持消息记录与传输的现有持久化顺序；首次恢复不能造成重复可见气泡或绕过 prompt 去重。
7. 已有业务数据与 NotRequested 矛盾时不得 New。真实非空 timeline / usage 可作为“禁止创建”的一致性拦截，只需固定文件 metadata，不扫描正文；日志不存在不能反向证明安全。
8. 身份、创建授权和生命周期使用同一 ACP 存储适配层。不能临时另建一个 session-state 文件与 snapshot 双写。
9. 可靠提交复用现有 atomic-write-file / write_json；实施时验证所用库在目标平台的原子替换与持久化保证，必要时在统一存储适配层补足 flush / sync，不在调用点自写临时文件协议。

### 6.4 Stop、迟到响应与身份写入

创建授权提交是本地发送权的线性化点：

- Stop 先完成：CAS 失败，不创建 session。
- 授权先完成：允许最多一个已获授权的创建请求继续收敛；Stop 不承诺撤回已经在途的外部创建。
- Stop 后即使收到合法 sessionId，也应按创建授权中的 operationId 提交这一 Attempt 级身份事实；它不代表本轮重新 Running。
- 身份提交必须使用专门的短事务：核对创建授权与身份一致性，只更新身份及必要连接元数据，保留当前 lifecycle header、turnError、revision 和取消结果。
- 不能直接调用旧 turn 的整份 metadata rewrite：旧 owner 已失效时它可能丢弃身份，或把 cancelled 覆盖成 running。
- 已取消或被替代的旧 turn 不能继续发送 session/prompt；下一轮拿到该身份后正常 Continue。
- 新 turn 已接收而旧请求尚未释放执行通道时，新 turn 等待原通道收敛；进入后重新解析，不能沿用锁外的 Create 结果。

这保证本地不会因为迟到事实重复授权创建；不保证 Provider 的创建行为具有跨系统事务或 exactly-once 语义。

### 6.5 故障窗口处理矩阵

| 故障位置 | durable 事实 | 下一次发送 |
|---|---|---|
| initialize 前 / 期间停止或崩溃 | NotRequested，无身份 | 满足 Direct 资格后允许首次创建 |
| 创建授权提交失败 | 未取得授权；可能保持 NotRequested | 修复存储问题后重新判定，不能假称请求已发送 |
| 授权成功、请求尚未送达就崩溃 | Requested，无身份 | 不自动 New，进入显式恢复 |
| 远端创建成功、响应丢失 | Requested，无身份 | 不自动 New，进入显式恢复 |
| 收到 sessionId、身份提交失败 | Requested，无身份 | 不自动 New，明确持久化失败 |
| 身份已提交、worker-ref 未提交 | 有一致身份 | 修复投影后 Continue |
| worker-ref 成功、首条 Prompt 尚未发送 | 有一致身份 | Continue，原用户轮次去重规则继续生效 |
| Stop 后收到迟到创建成功 | 身份可提交，turn 保持 cancelled | 下一轮 Continue，不复活旧 turn |
| 旧 metadata / config 回写晚到 | 最新身份与创建授权已存在 | 不允许清除或回退事实 |
| 旧格式无事实、无身份 | Unknown | 保留错误，显式重新发起 |

## 7. Direct 自动恢复资格与其他模式隔离

### 7.1 业务资格

App 在预检与最终执行边界复用现有校验，至少保证：

- Task run mode 为 Direct，locator 为普通 Attempt，非 Dynamic inner。
- 完整 project / task / run / round / node / attempt locator 与 canonical current 一致。
- Run / Round / Node 先通过现有结构校验；Run 使用 `is_run_continuable`，复用 outcome、pause reason 与 current 检查。
- Run / Round / Node 生命周期互相一致；Node 为可继续暂停、无 outcome、无 manual check pending、无运行中的旧 runtime execution。
- 目标不只读、未被 successor 接续而替代，不处于 permission / manual check / error-blocked 等禁止通用恢复的干预态。
- 不存在仍占有执行权的其他 Provider prompt。

校验必须区分“旧执行仍活跃”和“本轮已经 admission / claim”：进入执行函数时，本轮 lifecycle 本来就是 Starting / Accepted，不能要求整个 snapshot 为 Idle，否则会把自己的合法恢复请求挡住。

### 7.2 解析规则

1. 有一致、可信、可恢复身份：Continue，必要时修复派生引用。
2. 没有身份且显式 NotRequested，业务资格通过、无矛盾持久数据：CreateDirectSession。
3. Requested、Unknown、损坏、身份冲突或资格不通过：Blocked。
4. 桌面 Dynamic 调用路径只提交严格 Continue，不复用允许 Direct Create 的入口。
5. Workflow / AUTO 的显式 runtime continue 继续严格恢复；不因 unavailable 或 turnError 自动新建。
6. 既有显式 New 与自动重试入口必须逐项审计：普通 Direct 在已有身份时续接，在 Requested + no identity 时停止自动重试；其他模式按各自已授权的创建语义执行，不机械套用 Direct 的单次首次创建规则。

## 8. Prompt 内容与输入恢复契约

### 8.1 SessionMode 只负责传输

修正 RawAgent 通过 SessionMode 选择 requirement / resume_prompt 的隐式耦合，正文应由明确的 UserPromptRenderMode 决定：

| 内容语义 | Agent 实际收到 | UI 展示 |
|---|---|---|
| RequirementTask | 正常首轮需求及既有首轮上下文 | 正常首轮消息 |
| UserMessage | 本轮正文、引用与角色输入 | 本轮输入 |
| DirectSessionBootstrap（新增显式语义） | 原始需求上下文 + 本轮当前消息 + 必要首轮输入 | 仅本轮输入及其附件 |
| RuntimeResume / Repair / Finalize 等 | 既有对应模板 | 既有可见性规则 |

不能让 New + UserMessage 产生空 Prompt，也不能改成 RequirementTask 后丢掉本轮消息。对附件-only 输入，允许正文为空但必须有合法内容块；校验的是有效请求，不是强制非空字符串。

### 8.2 DirectSessionBootstrap 内容

仅在最终目标为 CreateDirectSession 时构建：

1. 从当前 Task 的冻结输入 / Run 关联需求读取原始 requirement、引用、角色、workspace 文件和首轮隐藏区段。
2. 从当前 durable prompt submission 读取本轮完整 ConversationPromptInput 与附件；不能从前端当前草稿或旧组件状态重建。
3. 原始需求作为前序上下文，本轮消息作为当前指令；两者冲突时按本轮当前指令处理。用户停止的旧操作不能因为重建而自行重放执行。
4. Agent 收到完整原始上下文与本轮消息；不把多次失败 submission 累积成无限历史。
5. display_text、可见 quotes / role 与本轮输入一致，timeline 只创建本轮一个可见消息，不把原始需求重复伪装成本轮新发送的第二个气泡。
6. 首轮隐藏区段按现有已批准的 Direct 首轮投影策略重发，不能沿用普通追问中的 `extra_hidden_sections.clear()`。
7. Runtime 的路径、权限边界与协议约束仍按项目 system/user 标准管理；不得为了恢复向 RawAgent 注入整套 Gold Band runtime system prompt。

若缺少必要原始输入或已声明附件不可读取，返回具体输入错误；不能静默发送空 Prompt、借其他 Task 文件补齐或截断原需求。

### 8.3 附件、配置与去重

- 首轮附件与本轮附件通过现有 resolve / 内容投影链处理，遵守 canonical root、稳定引用和现有内容大小限制。
- 用现有 attachment identity 去重，不能只按文件名去重；明确区分 Agent 所需的原始附件与当前消息气泡的附件投影。
- 首轮引用、角色和 workspace 文件进入 Agent 上下文；本轮引用不能在先渲染后只改 display_text 时丢失。
- 模型、权限与 config options 使用当前 Attempt 已确认的会话覆盖值及冻结配置，不改成 Agent 的最新默认值。
- 补齐 worker-ref 时不重置用户已选择的模型或权限，不顺便执行 Doctor 或加载全量能力目录。
- promptId、operationId 继续使用 admission 分配的既有身份；准备失败或目标变化不生成第二个 turn。
- 对重复 turnId 的请求沿用既有 admission 幂等；不同的新消息不得被按正文相同误去重。

### 8.4 提示词管理

若需要新的组合模板，在以下七种语言的相同相对路径建立 `runtime/direct-session-bootstrap-user.md`：

`zh-CN`、`zh-TW`、`en`、`ja-JP`、`ko-KR`、`pt-BR`、`es`。

先读取并遵守 `docs/gold-band/rules/prompt-authoring.md`，使用现有模板渲染与语言降级，不在 Rust / TS 中拼接长指令文本。模板仅表达恢复输入语义，不承担 lifecycle 决策。

## 9. 失败详情投影与前端加载

### 9.1 后端保留结构化失败详情

普通与 Dynamic VM 使用一致规则：

1. 在提前返回 None 之前，将 turnError 解析为 RuntimeErrorInfo；key 存在、null 或畸形对象不等于有效错误。
2. unavailable、无 sessionId、无可展示历史、无 branch 时，只有同时没有有效 turnError 才维持占位 None 行为。
3. 有效错误返回现有 AcpSessionVm，保留失败状态、sessionId=null、真实 eventPage 与原结构化错误；不新建“错误会话”持久实体。
4. turnError 无效时不伪造业务错误或让整个会话树不可用；保留已有其他详情，并在诊断中记录格式问题。
5. 查询不修 worker-ref、不初始化创建授权、不扫描 Raw 来决定安全恢复。
6. sidebar / session tree 的 sessionEstablished 继续反映真实会话或历史，不因“返回了一个失败 VM”就变成 true。

不能仅把 VM 例外限定给 Direct 来隐藏 Workflow/AUTO 的错误；共享投影应保留事实，由 shell 按业务模式决定呈现。

### 9.2 分离两个现有判断职责

- **详情已就绪**：本次响应足够结束初始数据获取，包括“已确认空历史的有效失败”。
- **会话已建立 / 有可展示会话内容**：来自 canonical session availability、有效身份、已加载真实内容或既有明确 metadata 契约；单独 turnError 不成立。

继续复用现有 shell resolver，不另建一套页面生命周期。

readiness 增加失败详情分支时，至少要求：

```text
status == failed
&& turnError 有效
&& 当前 scope / generation 响应被接受
&& 已加载可展示历史，或本次详情确认 eventPage.total == 0
```

- total > 0 且 events=[] 时继续首批分页加载，不能提前显示空聊天。
- 事件是否存在以完整分页字段和既有可展示事件规则为准，不用任意 metadata 事件冒充聊天内容。
- 分页失败时保留已知错误与局部重试入口，不清空已显示内容。
- 只对仍在进行、尚未产生可用详情的初始化继续既有有界重试；已确定失败不再耗尽约 32 秒 missing-session 重试。
- 读取失败与 Provider turn 失败分别处理，不能把网络错误误判成一个空历史终态。

### 9.3 模式与 shell 优先级

修正 `isAcpSessionInitializationFailed / Interrupted` 对 !baseSessionReady 的依赖，改为检查“尚无真实会话或可展示内容”，避免错误详情 ready 反过来取消初始化 shell。

1. 当前 scope 的 canonical 权限、只读与 Runtime 控制状态优先。
2. Workflow/AUTO 没有真实 session / timeline / 明确可展示 metadata，且初始化已失败或中断：展示既有错误或中断页面，隐藏普通聊天 composer。
3. Workflow/AUTO 已建立或有真实历史：加载并展示历史；不能因为摘要没有内嵌详情而退回初始化 shell。
4. Direct 的安全未创建暂停可保留现有 cancelled shell；失败时展示具体横幅及允许的恢复操作。
5. Direct 结果不确定时，显示失败详情和“重新发起会话”，不假装普通发送能够原地恢复。
6. 健康空占位仍不提前 ready；停止 / 失败终态不能被旧 Starting、旧 VM 或旧 live event 回退。
7. 新 turn admission 后旧 turnError 按既有 lifecycle 转换清理；新失败可重新显示，不能靠清除历史文件隐藏旧错误。

## 10. 历史数据与结果不确定时的恢复

### 10.1 不做推断式迁移

- 不根据 Raw 没有 session/new、文件不存在、文件为空、时间戳先后或诊断文本补写 NotRequested。
- 有可信身份的旧数据继续恢复，并可在写路径修复派生引用。
- 无身份且无创建授权事实的旧数据始终 Unknown；包括本次现场材料，除非还有可验证的权威事实，不能承诺自动原地恢复。
- 不启动扫描全部 Task / Run / Raw 的迁移；仅对当前用户操作的目标进行读取或修复。
- 旧数据的读取走现有存储归一化路径，不再增加一套 legacy fallback 模型。

### 10.2 显式恢复操作

在 Direct 失败区域提供“重新发起会话”，复用现有重新运行会话 Task / 创建新 Run 的命令和返回值：

1. 用户明确触发后创建新的 canonical Run / Attempt，使新旧 Provider 会话身份天然隔离；不在旧 Attempt 上清空 Requested 后重发。
2. 保留旧 Run、错误、timeline、Raw、附件和创建事实，不删除原引用。
3. 新 Run 正常使用原 Task 需求、配置和输入。用户尚未提交或发送失败后保留的当前草稿仍保留，不自动加入原始首轮或暗中重复发送。
4. UI 明确显示已经切换到新的 Run；不要把新会话伪装成旧会话已恢复。
5. 不提供自动带上当前草稿再次发送的第二套流程；用户在新会话中按普通发送操作提交即可。
6. 该动作按现有 rerun 命令处理进行中状态与重复点击；补齐目标级防重入，禁止双击生成多个新 Run，不为此新建持久队列。
7. 无需再要求用户确认一次纯粹的新 Run 创建；只有复用入口确实会停止其他活跃执行时，按既有冲突交互处理，不顺带停止无关运行。
8. 结果不确定时可能遗留远端空会话，客户端不按名称猜测并关闭它；未来仅在 Provider 有明确可验证能力时另行设计收敛。

界面文案只说明用户可执行的操作及业务后果，不展示创建授权、CAS、Raw 等实现术语。

### 10.3 结构化错误

- 缺引用沿用 `acp.session-restore-reference-missing`。
- 根据最终错误目录补齐有类型的错误变体，至少区分创建结果不确定、身份冲突、创建事实损坏和身份持久化失败。
- 对应稳定 code 建议为 `acp.session-creation-uncertain`、`acp.session-identity-conflict`、`acp.session-creation-state-invalid`、`acp.session-identity-persist-failed`；实施时先查重并统一注册，不能在各调用点散落字符串。
- 错误复用 RuntimeErrorInfo 的 code、结构化 params、recovery 与 diagnostic；后端不生成对客文案，前端按 code 本地化。
- 保留底层可诊断原因，但不把绝对路径、协议全文等实现细节堆进用户主提示。
- 新增动作和错误文案同步七语言，遵守 ui-locale.md。

## 11. 停止、重启与 Prompt Queue

- 初始化期间继续允许停止，不以“首个回答前禁用停止”规避恢复缺陷。
- 新增 durable 创建授权不接管 Run / Node 生命周期；取消与失败仍由既有 owner / revision 终态策略收敛。
- 应用退出或 Provider 重载继续复用既有 draining、cancel、bounded shutdown，不增加无限等待。
- 重启恢复在既有目标恢复路径中收敛遗留活跃 turn；NotRequested / Requested 与 sessionId 原样保留，不在普通详情查询中执行维护写入。
- Prompt Queue 保留现有 durable 结构与顺序，通过同一 submit / admission / 最终目标解析入口执行。
- bootstrap 准备或创建失败，队列按既有失败策略暂停排空，保留尚未实际接受的用户输入；不能把创建成功当成用户 Prompt 已发送成功。
- 重复投递同一 turn 不重复 session/new 或消息；下一条不同 turn 重新读取最新身份与 owner。
- 新建 Run 的显式恢复不自动迁移旧队列，旧输入可审计、可由用户管理，不暗中批量重放。

## 12. 验证方案与验收证据

### 12.1 先红后绿

修改实现前，先针对当前实现建立稳定最小失败测试，并保存失败断言与状态：

1. Direct initialize 早停后的后续发送错误选择 Continue。
2. RawAgent New + UserMessage 的实际 user_prompt 丢失当前输入。
3. unavailable + 有效 turnError + 空历史被投影为 None。
4. 失败详情 readiness 不接受真实空历史。

另外先固定以下保护性场景，再用于约束新方案；它们不必在当前实现上失败，不能冒充已复现的现有 Bug：

1. Workflow/AUTO 初始化 shell 在没有真实会话时仍为失败或中断，防止新增 readiness 分支后错误变为 available。
2. 目标预判与进入执行通道之间出现迟到身份，最终只能 Continue，不能沿用旧 Create 计划。
3. session/new 已交给 transport 而 Raw 写入失败，后续恢复不能依据 Raw 缺记录再次创建。旧方案的 Raw resolver 尚未实现，不能把它描述成当前已有的错误分支。

新增字段的状态转换与崩溃用例可以随后建立契约测试；不能把“缺少新字段导致 fixture 解析失败”当成当前 Bug 的复现。报告分别标注当前缺陷的红绿证据与新增不变量的契约验收。

### 12.2 ACP 存储与目标解析测试

- 只有真正的新 Direct Attempt 初始化 NotRequested；旧记录字段缺失、null、未知或损坏均不变成 NotRequested。
- 创建授权写失败时 fake adapter 的 session/new 调用次数为 0。
- 同一受保护 Direct 创建事实最多授予一次发送权；失败、取消、队列重投或重启不重置。
- 有效引用走 Continue，目标解析不读取 Raw、timeline 正文、全量 usage。
- sessionId 别名、空值遮蔽、两个别名冲突、snapshot / worker-ref 冲突分别固定。
- 有身份但缺 worker-ref 可从合法配置修复；cwd / adapter / project 不匹配时失败。
- metadata / title / config / terminal 重写不能删除创建事实或覆盖新身份。
- NotRequested 与非空历史矛盾时阻止自动 New，不做正文回扫。
- 非 current、只读、manual check、active Runtime、Dynamic / Workflow / AUTO 严格路径不能取得 Direct 创建授权。

### 12.3 确定性竞态与故障注入

使用 fake adapter、可注入存储失败和 barrier/channel 控制顺序，不依赖固定 sleep：

| 用例 | 必须断言 |
|---|---|
| 两个请求同时进入同一 Attempt | admission / owner 去重与执行串行成立，无重复自动创建 |
| 第一次锁外观察无身份，进入执行通道时已有身份 | 最终 Continue，使用 Continue Prompt，不发送 bootstrap |
| Stop 赢得创建授权前的 CAS | session/new = 0 |
| 授权赢得 CAS 后 Stop | session/new <= 1，取消 turn 不发送 session/prompt |
| 远端返回身份后 Stop 已终态 | 身份保留、cancelled 不回退，后续 Continue |
| 创建请求已发送但 Raw 追加失败 | Requested 保留，重启后不自动再次 New |
| 请求响应丢失 / 进程在各提交边界退出 | 按第 6.5 节矩阵恢复，不补造 NotRequested |
| 身份提交失败 | 不发送 Prompt，保留具体错误与不确定状态 |
| 身份成功、worker-ref 失败 | 下次修复引用并 Continue，无新建 |
| 旧 executor / metadata 晚返回 | 不覆盖新 owner、错误、身份或取消状态 |

进程退出窗口优先用子进程夹具验证原子提交后的重启结果；测试外部命令复用项目隐藏窗口创建方式，验证结束清理资源。

### 12.4 Prompt 接口验收

必须检查最终发给 fake adapter 的请求体，不只检查 display_text 或返回 SessionMode：

- bootstrap 含原始需求上下文与当前消息，当前指令优先。
- UI display_text 只等于本轮输入，timeline 只有一个本轮消息。
- 首轮 / 本轮附件、引用、角色、workspace 文件与隐藏区段完整且不重复。
- 普通 Continue 不重发原始需求或首轮隐藏区段。
- New + UserMessage 不再丢失正文；附件-only 合法发送。
- 模型、权限和 config options 沿用当前有效值。
- 原始输入不可用时明确失败，不发送空替代 Prompt。
- 新模板七语言路径与变量一致，RawAgent 不夹入 Runtime system prompt。

### 12.5 VM、前端状态与 DOM 测试

- 普通 / Dynamic 的有效 turnError 空历史保留；null / 畸形错误不能触发假失败 VM。
- total=0 的有效失败详情结束加载，不进入约 32 秒 missing-session 重试。
- total>0、events=[] 的失败详情继续首批分页，保留原有 terminal summary 测试。
- Workflow/AUTO 的初始化失败与中断仍显示对应 shell，DOM 中没有普通 composer。
- Workflow/AUTO 已建立历史先加载分页，不因详情尚未返回误判初始化中断。
- Direct 安全未创建暂停可继续输入；未知状态显示重新发起操作与具体原因。
- 结构化错误文案不退化为通用 missingSessionReason。
- 新 turn 清除旧错误；旧 scope / generation 响应不覆盖新 turn。
- “重新发起会话”重复点击只调用一次；新 Run 导航正确，草稿保留，旧队列不重放。
- Composer 输入或错误关闭不引起无关历史树重渲染，不扩大 Context 订阅。

### 12.6 命令、构建与客户端验收

实施时执行并记录：

1. 受影响 gold-band library、desktop 命令、ACP fake adapter 的定向 Rust 测试。
2. `acp-chat-events`、`acp-session-shell` 及新增 DOM / API 契约 Web 测试。
3. TypeScript 检查、生产前端构建、项目要求的 Rust 检查。
4. 按规则先尝试内置 iab，不可用时再用连接的 Chrome，最后才用 agent-browser；deep link 到目标会话。
5. 验证 Direct 初始化停止后重发、历史具体报错、结果未知后的重新发起、第二条 Continue 与重启恢复。
6. 验证 Workflow/AUTO 初始化失败 / 中断与正常历史；Dynamic 严格缺引用不 New。
7. 涉及真实停止或退出的客户端级行为时，补 EXE 验证，不能只用浏览器 mock 代替。
8. 清理本次页面、fake adapter、测试进程、临时 workspace 和临时文件。

此前已有测试通过只是基线，不代表上述新增契约已经验收。本方案所有实施验收项当前均未完成。

## 13. 代码与文档影响范围

### 13.1 预计代码修改

| 模块 | 职责 |
|---|---|
| `src/acp/events.rs` | 创建事实类型与短事务、metadata 合并保留、身份提交与 owner/CAS |
| `src/acp/session_target.rs`（建议）及模块导出 | 统一引用归一化、物化事实解析与有类型目标 |
| `src/acp/client.rs` | 锁内最终解析、创建授权、早期身份提交、取消与迟到响应 |
| `src/acp/connection.rs` | 核实传输发送边界；仅在需要暴露有类型发送结果或测试注入时修改 |
| `src/app/mod.rs`、`node_executor.rs`、普通 Attempt 实际创建写入点 | 新 Attempt 初始化、业务资格、按目标准备 Prompt |
| `src/provider/mod.rs` | 消除传输方式对内容语义的隐式决定、bootstrap 渲染、Direct 自动重试审计 |
| `src-tauri/src/commands.rs` | 普通与 Dynamic 调用意图分离，复用 admission 和准备 factory |
| `src-tauri/src/view_models.rs` 及 conversation 摘要投影 | 有效失败详情、统一身份与 sessionEstablished |
| 现有 rerun 命令、API、恢复 UI | 复用新 Run 入口，补目标级重复点击保护与导航 |
| `web/src/components/acp/ACPChatDialog.tsx`、`web/src/lib/acp-session-shell.ts` | 分离加载 ready 与会话 established，保留各模式 shell |
| `src/prompts/<language>/runtime/`、`web/src/locales/` | bootstrap 模板、结构化错误和恢复操作的七语言文案 |
| 现有结构化错误目录、Rust / Web 测试 | 注册错误变体，固定发送内容与持久化 / 并发不变量 |

模块清单是影响面，不要求机械修改每个文件。实施不得为了维持原先“四个文件”限制，把领域逻辑塞进桌面命令或 UI。

### 13.2 实施时必须同步的文档

1. `产品设计文档/interaction/app/conversational-runtime.md`：安全未创建、未知与已有身份的用户行为；历史恢复动作；各模式初始化 shell。
2. `产品设计文档/runtime/control.md`：session identity、创建授权、控制权正交、取消与迟到身份提交。
3. `开发计划/acp接入/acp-ui.md`：有错误的无 session 详情、分页 readiness 与 shell 区别。
4. `开发计划/direct模式/Direct模式完整设计与开发方案.md`：修正 8.2 等“一律 Continue”表述，补 bootstrap 内容与恢复边界。
5. `开发计划/生命周期整理/工作流停止后Runtime控制与自由会话统一技术方案.md`：共享接口影响、严格恢复与终态不回退。
6. `开发计划/gold-band-mvp-plan.md`：真实红绿证据、实施范围、客户端验收与发布版本。
7. 与本方案相关的 ACP 存储 / 错误目录说明：增加新字段、错误与不确定状态；若已有同等章节则直接维护原真源。

影响评估文档保留其原版本评审背景；本方案明确吸收实际问题，不把评估中的可选类型设计全部变成强制实现要求。

## 14. 实施顺序与交付要求

1. 建立第 12.1 节失败证据；核对所有普通 Direct 初始执行、手动追问、队列与自动重试入口。
2. 先实现 durable 创建事实、身份归一化与合并保留，完成存储 / 崩溃契约测试。
3. 把最终解析与创建授权移入既有执行通道，补早期身份提交及迟到响应处理。
4. 实现按最终目标构建的 Prompt，补 bootstrap 模板与实际请求体测试。
5. 完善全模式失败 VM、前端 ready / established 分离和初始化 shell 回归。
6. 接通显式新 Run 恢复动作、错误本地化与重复点击保护。
7. 完成定向测试、生产构建、真实客户端验证与文档同步，最后发布。

顺序用于降低实施风险，不是拆成只上线错误横幅的半成品。完整验收前不得把整个需求标记完成。

## 15. 方案自评审

### 15.1 过度设计评审

结论：一项新增 durable 事实有必要；其余复用现有机制。

- 现有 identity / revision / lifecycle 无法表达“没有创建过”与“创建结果丢失”，这是新增事实的具体依据。
- 新事实放回已有 snapshot，由同一 ACP 领域写入；不新增 aggregate、会话主键、生命周期状态机、独立日志库、队列、缓存或后台修复服务。
- Continue 引用类型、目标枚举和 Prompt render mode 只封装真实接口不变量，不为假设性 Provider 能力建扩展框架。
- 保留现有 AtomicWriteFile、serde、owner/CAS、attempt prompt 通道、单实例机制、模板渲染和 shadcn/prompt-kit 错误 / 按钮组件。
- 不需要新开源组件解决项目特定恢复规则；原子提交、结构化序列化与基础 UI 均已有成熟依赖，优先完善它们的使用契约。
- 不能因用户允许完整修复而顺便重构整个 ACP、Runtime、历史分页或调度框架。

### 15.2 性能影响评审

- 数据规模：每次恢复只涉及当前一个 Attempt 的固定小元数据、一个原始 requirement 与现有输入上限内的附件；不会随 workspace 总历史增长。
- 目标解析复杂度 O(1)，不读 Raw 正文、不全量扫描 timeline / usage，不引入 N+1、诊断回扫、能力发现或全局启动迁移。
- 普通 Continue 不构建 bootstrap，不新增创建授权写入；既有恢复链可能进行的 usage 修复应独立计量，不能宣称整个调用链永不读历史。
- 受保护的首次 New 增加固定次数的授权与身份可靠提交；Provider 网络等待与附件处理不占 metadata / workspace 短锁。
- 文件与解析操作进入既有 blocking worker，不阻塞 Tauri 事件线程或 UI 主线程。
- 前端使用现有 generation / revision 和分页水位，不增加轮询、不扩大全局状态订阅；确定失败消除无效长重试。
- bootstrap 内容增加只发生在首次安全恢复，沿用已有输入与附件边界，不累积所有失败消息，不做无界缓存。
- 验收固定文件访问次数、无 Raw / 全历史读取、锁外网络等待和失败查询次数；记录新增原子写入耗时与正常 Continue 准备耗时。
- 当前不预设存在显著性能瓶颈，不强制无意义 benchmark；若实测磁盘提交或准备路径形成明显退化，再按规则记录规模、基线和目标并专项验证。

## 16. 发布、旧数据与回退

- 本次包含 snapshot 新事实，不再沿用旧方案“无持久化变化”的发布说明。
- 不对历史数据补造 NotRequested，不进行全量迁移；字段缺失的历史按 Unknown 保守恢复。
- 不删除旧 worker-ref、snapshot、timeline、Raw 或 diagnostics，不重写历史失败来伪装修复成功。
- 上线前保留测试现场与必要快照备份，验证正常启动、崩溃重启和旧数据读取。
- 不让不同版本同时写同一 workspace。旧版本可能丢弃未知字段，回退后重新升级时字段缺失必须仍是 Unknown，不能默认允许 New。
- 回退应保留新事实与身份；不能通过删字段、清空 session 或重置创建状态恢复旧行为。
- Provider 已创建但身份永远丢失的远端会话，明确保留为未知外部结果；不承诺无条件清理，也不自动重复业务执行。

## 17. 完整验收清单

- [ ] 新 Direct Attempt 在任何 Provider 副作用前可靠初始化创建事实。
- [ ] NotRequested 与 Unknown 可区分，历史缺失事实不会被补成“从未请求”。
- [ ] session/new 前创建授权已提交，授权写入失败时无协议副作用。
- [ ] Requested + no identity 不会自动重发，重启、重投与 Raw 失败均成立。
- [ ] 有可信身份时 Continue；派生引用缺失可修复，身份冲突明确失败。
- [ ] Stop 先于授权可阻止创建；授权后的迟到身份可保存且不复活旧 turn。
- [ ] 最终目标在既有执行通道内决定，目标变化时正文与附件随之正确选择。
- [ ] bootstrap 实际请求包含原需求上下文与本轮消息，UI 只显示本轮消息。
- [ ] 附件、引用、角色、workspace 文件、首轮隐藏区段与配置均有请求体测试。
- [ ] 有效 turnError 不被 VM 丢弃，真实空历史首次有效详情后结束加载。
- [ ] 有历史未加载时不提前空白 ready，scope / generation 与终态单调成立。
- [ ] Workflow/AUTO 初始化失败 / 中断 shell 与已建立历史均回归通过。
- [ ] Dynamic 和严格 runtime continue 不会误走 Direct 自动创建。
- [ ] 历史未知与创建结果未知均有显式新 Run 恢复入口，保留旧数据与草稿。
- [ ] Prompt Queue 去重、排空与失败策略保持正确，不暗中迁移或重放。
- [ ] 新错误与模板七语言齐全，没有后端对客文案或长 Prompt 硬编码。
- [ ] 红绿证据、故障注入、Rust / Web 测试、类型检查与生产构建完成。
- [ ] 内置浏览器与必要 EXE 验证完成，测试资源已清理。
- [ ] 产品设计、开发计划、错误 / 存储说明和发布记录同步。
- [ ] 完成后重新进行过度设计与性能影响自评审，记录真实结果。
