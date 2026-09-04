# 客户端直连 IM 远程干预实施计划

## 1. 目标与交付边界

本计划实现 Gold Band 桌面客户端进程内直连企业微信，通过安装级全局一对一私聊处理已有 Runtime 的权限审批、用户追问和人工成功/失败判定，并投递用户逐类启用的 Run 与 ACP turn 信息通知。微信等待稳定主动推送真实 PoC，不部署 Gold Band 云端网关。

产品契约见：[客户端直连 IM 的远程干预设计](../../产品设计文档/integration/im-remote-intervention.md)。

完成定义：

- 企业微信可在设置页扫码授权、启停和查看状态；连接 generation 与结构化错误直接反映真实连接结果，微信不出现设置入口。
- 远程动作和桌面动作共用领域服务，满足完整 locator、revision、幂等和 first-writer-wins。
- 凭据不落入设置文件、SQLite、日志或前端状态。
- 网络断开、限流、重连、重启和重复回调有确定行为。
- 每一阶段都有接口级单元测试；UI 阶段完成浏览器验证，平台阶段完成真实沙箱/测试账号验收。

## 2. 现状基线与改造原则

现有可复用能力：

- `src/app/mod.rs` 已定义 `RuntimeLifecycleEvent::InterventionRequested`，包含稳定 event ID、时间和完整项目/任务/运行/轮次/节点/attempt 定位信息。
- `src/app/observability.rs` 的 `RuntimeLifecycleBus` 已支持具名异步订阅者。
- `src/app/notification.rs` 与 `src-tauri/src/notifications.rs` 已实现系统原生通知投影、点击导航和进程内提醒去重。
- `src-tauri/src/commands.rs` 已有 `respond_acp_permission`、`respond_elicitation`、`submit_manual_check`。
- `src-tauri/src/state.rs` 的 `DesktopState` 管理长生命周期桌面服务。
- `GoldBandPaths::core_db_path()` 已提供用户级 `~/.gold-band/core.db`，数据库已使用 WAL、`synchronous=FULL` 和 component schema marker。
- `SettingsConfig` 当前 schema version 为 10，设置写入已有统一校验、迁移和原子落盘入口。

改造原则：

1. 先抽取领域写接口，再接任何 IM 平台。
2. Runtime canonical state 不变，IM 不新增审批状态机。
3. lifecycle subscriber 只做有界入队，不做网络请求。
4. 平台协议 DTO 只存在于 connector 内。
5. 设置变更使用 generation 替换连接；迟到旧连接事件不能覆盖新状态。
6. 当前为开发阶段，不保留旧设置结构的双读和 fallback；schema migration 一次完成。

## 3. 依赖选型

### 3.1 必选通用依赖

- `keyring 4.2.0`：MIT OR Apache-2.0，MSRV 1.88；调用 Windows Credential Manager、macOS Keychain 和 Linux Secret Service。按目标平台只开启原生 backend，并验证目标平台行为。
- `tokio`：复用现有 runtime，负责连接任务、取消、超时和有界 channel。
- `serde` / `serde_json`：复用现有结构化协议。
- `rusqlite`：复用现有 `core.db` 基础设施。
- `qrcode 1.5.4` / `@types/qrcode 1.5.6`：MIT、成熟且被 Halo 同一流程使用；仅在扫码 Dialog 动态加载并绘制 Canvas，不进入设置首屏主包热路径。

### 3.2 平台 SDK 决策门

| 平台 | 候选 | 2026-08-30 审计结论 | 合入/发布门槛 |
|---|---|---|---|
| 企业微信 | `@wecom/aibot-node-sdk 1.0.6` 协议基准 / `wecom-aibot-rust-sdk 1.0.2` / `tokio-tungstenite 0.30.0` | 官方 SDK 为 MIT、由 `WecomTeam` 维护，Halo 也通过其 `WSClient.sendMessage` 使用主动推送；Gold Band 不引入 Node helper。拒绝 Rust SDK：无 MSRV、采用量低且重复引入 reqwest 0.11、tokio full、tungstenite 0.21/native-tls；采用 `tokio-tungstenite` 薄封装官方 JSON 协议，MIT、MSRV 1.85、维护活跃 | fixture 固定官方 `chatid + payload` 主动推送帧、顶层 `errcode` ACK、鉴权、心跳、卡片回调/update、限流和重连；真实机器人验收仍是发布阻塞项 |
| 微信 | 官方 `@tencent-weixin/openclaw-weixin 2.4.6` / `weixin-agent 0.3.0` | 延期，不进入生产依赖；官方包 MIT，但主动推送与 context token 稳定边界未经真实 PoC 证明 | 真实账号证明无需用户先发消息即可稳定主动推送、休眠恢复与长期重连后重新评审；禁止 pull-only 降级 |

不得为了“统一 SDK”编写自有网络框架。若官方/成熟 SDK 能满足协议与生命周期要求，connector 应薄封装；只有候选未通过审计的单个平台才退回成熟传输库加官方协议实现。

当前工具链基线是 Rust/Cargo 1.95。所有新增 crate 必须通过 `cargo tree` 检查重复 TLS/runtime 依赖、许可证和目标平台构建。企微 vote 协议闸门已用本机真实账号完成手机端/PC 端受控 PoC；微信能力仍不得把 fixture/接口测试记录成真实平台验收。

企业微信扫码授权采用官方 `@wecom/cli` 已公开的 device-flow：`generate` 取得 `scode/auth_url`，每 3 秒查询 `query_result`，5 分钟到期。`source` 是安装级产品来源标识，写入 `configs/app-config.toml` 的 `[im.wecomScanAuth]`；当前按用户明确决定临时设置为 `halo`。官方资料未找到该标识的公开申请/分配规则，因此真实企业范围能力仍是平台验收阻塞项，不把当前可生成二维码等同于正式授权资格。

## 4. 模块布局

建议新增和调整以下文件：

```text
src/app/
  intervention.rs                 # 领域写服务与结构化结果
src/im/
  mod.rs                          # 公共 facade
  model.rs                        # typed payload、设置和 delivery 模型
  projection.rs                   # lifecycle/scheduled -> outbox projection
  connection_manager.rs           # generation、启停和状态广播
  connector.rs                    # ImConnector trait、能力和错误
  inbound.rs                      # action token、入站幂等和 Runtime 调用
  repository.rs                   # core.db component repository
  credential.rs                   # OS credential store adapter
  worker.rs                       # bounded delivery worker
  connectors/
    wecom.rs

src-tauri/src/
  im_runtime.rs                   # DesktopState 装配、commands 和 typed event bridge
  state.rs                        # 增加 ImConnectionManagerHandle
  commands.rs                     # lifecycle 注册、领域 command 委托、退出编排

web/src/
  api/client.ts                   # public API contract
  api/desktop.ts                  # Tauri invoke 实现
  pages/SettingsPage.tsx          # 集成设置 section
  components/settings/
    ImIntegrationSettings.tsx
  i18n.ts                         # zh-CN/en 错误和 UI 文案

web/tests/
  im-integrations-settings.test.tsx
  im-api-contract.test.ts
```

现有 `commands.rs` 只保留 command 注册/委托，新增 IM commands 优先放独立 `im_commands.rs`。不得为了此功能单独做无关的大规模 commands 重构。

依赖方向固定为：

```text
platform connector -> im public contract -> InterventionCommandService -> Runtime
Tauri command ------^                       ^
                                            |
desktop UI existing intervention commands -+
```

`src/app` 不依赖 Tauri。连接器不得调用 `respond_*` Tauri wrapper，也不得构造 `tauri::State`。

现有系统原生通知与新 IM service 是同一个 `RuntimeLifecycleBus` 上的具名兄弟订阅者：前者保持一次性提醒和点击导航语义，后者负责可靠投递和远程动作。不得把 `NotificationDedup` 扩展成 IM 幂等账本；其“用户关闭后清 key”的语义与外部平台重复回调冲突。两条投影应共享稳定 event ID 和 intervention kind 转换，避免重复发明类型映射，但不共享投递状态。

## 5. 领域接口先行

### 5.1 统一命令服务

从现有三个 Tauri command 中提取共享内部服务：

```rust
pub struct InterventionCommandService {
    app: Arc<App>,
    // 仅持有现有领域入口所需依赖，不复制状态。
}

pub enum InterventionCommand {
    RespondPermission {
        locator: InterventionLocator,
        request_id: String,
        option_id: String,
        expected_revision: u64,
    },
    RespondElicitation {
        locator: InterventionLocator,
        elicitation_id: String,
        action: ElicitationAction,
        content: Option<String>,
        expected_revision: u64,
    },
    SubmitManualCheck {
        locator: InterventionLocator,
        outcome: ManualCheckOutcome,
        expected_revision: u64,
    },
}

pub struct InterventionCommandContext {
    pub source: InterventionSource,
    pub actor: InterventionActor,
    pub action_id: ActionId,
}

pub enum InterventionCommandResult {
    Accepted { revision: u64, resolved_at: DateTime<Utc> },
    AlreadyApplied { revision: u64, resolved_at: DateTime<Utc> },
}
```

调用流程固定为：

1. 校验完整 locator 和 project/workspace trust boundary。
2. 加载当前 authoritative request 与 revision。
3. 校验 request ID、类型、owner、允许动作和未过期状态。
4. 在现有领域写锁/原子状态转换内执行 compare-and-set。
5. 返回结构化结果和新 revision。
6. 写入审计结果；审计失败不得回滚已生效的 Runtime 决策，但必须记录可恢复诊断。

Tauri 原有三个 command 改为构造 `InterventionCommand` 并委托服务，保持前端 API 行为。IM action handler 使用同一服务。不得先让 IM 调用旧 wrapper 再后补抽象。

### 5.2 接口级测试

必须先固化以下测试：

- 权限、追问、人工检查分别只进入对应领域入口。
- 完整 locator 任一字段错误均拒绝。
- outer locator 在 AI-DYNAMIC 等复合场景中参与校验。
- 非当前 owner 的历史 attempt 不可提交。
- 非 `manual_check_pending` 不可提交 success/failure。
- 相同 revision 的两个并发终局动作只有一个 accepted。
- 同 action ID 重试返回 `AlreadyApplied`。
- 桌面来源与 IM 来源遵守同一状态不变量。

## 6. 公共 IM 接口与数据结构

### 6.1 Connector trait

```rust
#[async_trait]
pub trait ImConnector: Send + Sync {
    fn kind(&self) -> ImChannelKind;
    fn capabilities(&self) -> ImChannelCapabilities;

    async fn connect(
        &self,
        config: ResolvedImChannelConfig,
        generation: ConnectionGeneration,
        events: mpsc::Sender<ImConnectorEvent>,
        cancellation: CancellationToken,
    ) -> Result<(), ImIntegrationError>;

    async fn send(
        &self,
        delivery: ImDelivery,
    ) -> Result<ImDeliveryReceipt, ImIntegrationError>;

    async fn update(
        &self,
        binding: ImDeliveryBinding,
        state: ImMessageState,
    ) -> Result<(), ImIntegrationError>;
}
```

`connect` 是由 manager 管理的长生命周期任务。平台 SDK 的 client、event、message、card 类型不允许出现在 trait 或 `src/app/im/mod.rs` 中。

### 6.2 连接状态

```rust
pub struct ImChannelSnapshot {
    pub kind: ImChannelKind,
    pub enabled: bool,
    pub generation: u64,
    pub state: ImConnectionState,
    pub capabilities: ImChannelCapabilities,
    pub binding: Option<ImBindingSummary>,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<ImErrorCode>,
}
```

manager 每个平台仅保留一个 active generation 和一个 cancellation token。状态归并函数必须先比较 generation；旧 generation 事件直接丢弃。状态快照通过 Tauri typed event 增量发送，只供设置 section 订阅。

### 6.3 投递模型

```rust
pub struct ImDelivery {
    pub delivery_id: DeliveryId,
    pub channel: ImChannelKind,
    pub destination: ImDestination,
    pub notification_kind: ImNotificationKind,
    pub canonical_event_id: String,
    pub payload: ImDeliveryPayload,
}

pub enum ImDeliveryPayload {
    Intervention(InterventionRef, InterventionPresentation),
    Information(InformationalNotification),
}

pub enum ImConnectorEvent {
    Connected { generation: u64, identity: ImConnectionIdentity },
    Disconnected { generation: u64, error: ImIntegrationError },
    InboundAction { generation: u64, envelope: ImInboundEnvelope },
    BindingObserved { generation: u64, binding: ImBinding },
}
```

`InterventionPresentation` 是领域中立的标题、摘要、字段和允许动作，不包含最终平台 JSON。connector 负责渲染卡片或纯文本。

## 7. SQLite 设计

### 7.1 存储位置与 schema owner

IM transport 数据属于用户级跨 workspace 基础设施，存入 `~/.gold-band/core.db`。新增 component schema marker `im_remote_intervention`，由独立 repository 初始化和迁移。不得写入项目 `.gold-band` 目录。

### 7.2 表结构

```sql
CREATE TABLE im_outbox (
    delivery_id          TEXT PRIMARY KEY NOT NULL,
    channel_kind        TEXT NOT NULL,
    destination_id      TEXT NOT NULL,
    notification_kind   TEXT NOT NULL,
    canonical_event_id  TEXT NOT NULL,
    payload_kind        TEXT NOT NULL,
    payload_json        TEXT NOT NULL,
    state               TEXT NOT NULL,
    attempt_count       INTEGER NOT NULL DEFAULT 0,
    next_attempt_at_ms  INTEGER,
    expires_at_ms       INTEGER NOT NULL,
    last_error_code     TEXT,
    created_at_ms       INTEGER NOT NULL,
    updated_at_ms       INTEGER NOT NULL,
    CHECK (payload_kind IN ('intervention', 'information')),
    CHECK (state IN ('pending', 'sending', 'sent', 'expired', 'dead_letter')),
    UNIQUE(channel_kind, destination_id, notification_kind, canonical_event_id)
);

CREATE INDEX idx_im_outbox_due
ON im_outbox(channel_kind, state, next_attempt_at_ms, created_at_ms);

CREATE INDEX idx_im_outbox_canonical_event
ON im_outbox(canonical_event_id, notification_kind, channel_kind);

CREATE TABLE im_delivery_bindings (
    delivery_id          TEXT PRIMARY KEY NOT NULL,
    channel_kind         TEXT NOT NULL,
    platform_message_id  TEXT NOT NULL,
    platform_chat_id     TEXT NOT NULL,
    update_token_ref     TEXT,
    created_at_ms        INTEGER NOT NULL,
    updated_at_ms        INTEGER NOT NULL,
    UNIQUE(channel_kind, platform_message_id)
);

CREATE TABLE im_inbound_actions (
    action_id            TEXT PRIMARY KEY NOT NULL,
    channel_kind         TEXT NOT NULL,
    canonical_event_id   TEXT,
    actor_id_hash        TEXT NOT NULL,
    platform_event_id    TEXT NOT NULL,
    result_code          TEXT NOT NULL,
    result_revision      INTEGER,
    received_at_ms       INTEGER NOT NULL,
    completed_at_ms      INTEGER NOT NULL,
    UNIQUE(channel_kind, platform_event_id)
);

CREATE INDEX idx_im_inbound_actions_retention
ON im_inbound_actions(completed_at_ms);
```

约束：

- `payload_json` 使用版本化的 typed DTO 序列化，读取后必须再次做领域校验；`Intervention` 和 `Information` 不得使用 nullable action 字段互相兼容。
- `payload_json` 不保存 Secret、token、用户回答正文或完整用户对话。
- `update_token_ref` 只能是可安全持久化的平台消息引用；若平台 update token 属于敏感材料，则改存 keyring reference。
- `sending` 是租约态，需有 repository 方法在启动时把超时租约恢复为 `pending`；不可永久卡住。
- 查询 due outbox 使用索引并固定 batch size，默认每批 32 条。

### 7.3 原子顺序

出站：

1. lifecycle subscriber 以 `delivery_id = hash(channel + destination + notification_kind + canonical_event_id)` 幂等插入 `pending`。
2. worker 短事务 claim due rows 为 `sending`，提交事务后才调用网络。
3. 成功后短事务写 `sent` 和 delivery binding；失败后写 retry/dead-letter。
4. 发送后数据库写失败允许平台侧重复消息，但必须依靠稳定 delivery key/平台幂等能力和本地去重收敛，不能在锁内持有网络请求。

入站：

1. 仅 `Intervention` 可由 `(channel, platform_event_id)` 生成稳定 `action_id`；`Information` 不创建 inbound action。
2. 先查已有终态结果；存在则直接返回该结果。
3. 创建 processing reservation 或在 repository 提供的单 action mutex 下提交领域动作。
4. 领域动作成功后写结果；并发处理者只允许一个执行领域调用。
5. 崩溃发生在领域成功、审计未写之间时，重试必须由 Runtime revision/请求终态识别为已处理，而不是重复改变状态。

不得尝试在 SQLite 和 Runtime 状态文件之间建立跨存储事务。

### 7.4 容量与保留

- active outbox 硬上限 1,000；插入前用索引计数 active states，不执行全表 payload 加载。
- sent/expired/dead-letter 默认保留 7 天。
- inbound action 默认保留 30 天。
- 每次清理最多 200 条，低频执行；VACUUM 不放在应用正常运行热路径。
- 单条 presentation 最大 32 KiB，用户回答在进入领域服务前最大 8 KiB。

## 8. 设置 schema 与凭据

### 8.1 SettingsConfig v12

v11 将 `CURRENT_SETTINGS_SCHEMA_VERSION` 从 10 升到 11，并新增：

```rust
pub struct ImIntegrationSettings {
    pub channels: Vec<ImChannelSettings>,
}

pub struct ImChannelSettings {
    pub kind: ImChannelKind,
    pub enabled: bool,
    pub public_identity: ImPublicIdentity,
    pub credential_ref: Option<CredentialRef>,
    pub binding: Option<ImBindingSummary>,
    pub notifications: ImNotificationPreferences,
}
```

`ImNotificationPreferences` 固定包含 permission、elicitation、manual check、Run success、Run failure、ACP turn finished 六个 typed 开关。默认开启三类干预和 Run failure；默认关闭 Run success、ACP turn finished。scheduled completion、failure、attention 和 missed 不进入 IM 偏好与 outbox，继续由定时任务原生通知策略管理。

v10 -> v11 migration 只补默认 `im_integrations.channels = []`。v12 删除 scheduled completion、failure、attention、missed 四个 IM 偏好字段；v11 -> v12 migration 在严格反序列化前从每个 channel 的 notifications 中删除这些废弃键并原子写回，当前 v12 输入仍拒绝废弃键，不保留兼容消费路径。配置更新必须拒绝重复 platform、未知 platform、启用但没有 credential reference、无效 ID 和超长 display field。

前端 view model 不含 credential reference 的真实 key，只返回：

```ts
type ImCredentialStatus = 'missing' | 'stored' | 'invalid';
```

### 8.2 keyring 命名

固定 service：`com.gold-band.desktop.im`

固定 account：

```text
im/{channel_kind}/{installation_id}/{credential_id}
```

credential payload 是版本化 JSON，只包含该 connector 需要的敏感字段。`credential_id` 使用 UUID，不包含 Bot ID、App ID、用户 ID 或 workspace 路径。非敏感设置只保存同一个 UUID reference。

凭据写入流程：

1. 后端接收一次性 Secret，前端提交结束立即清空输入 state。
2. connector 对配置做本地结构校验；“测试连接”可执行最小远程验证。
3. 写入新 credential UUID。
4. 原子更新 settings reference。
5. 删除旧 credential；删除失败记录清理任务，但新配置保持可用。

设置更新失败时删除本次新建 credential，避免孤儿记录。读取凭据失败返回 `ImCredentialUnavailable`，不得把内容带入错误 details。

## 9. 生命周期与并发

### 9.1 启动顺序

1. `DesktopState` 打开并迁移 settings 与 `core.db` IM component schema。
2. 构造 `InterventionCommandService`、repository、credential store 和 connector registry。
3. 构造 `ImConnectionManagerHandle`，初始状态全部为 disabled/disconnected。
4. 在 `register_lifecycle_subscribers` 注册 `im-remote-intervention` subscriber。
5. 恢复 outbox 中超时 `sending` 租约，清理有界过期记录。
6. 桌面 shell 完成启动后，后台解析已启用配置并逐平台启动连接，不阻塞首窗显示。

`core.db` component 无法打开时仅禁用 IM 集成并发出结构化设置状态，不阻塞 Runtime 和桌面主路径。

### 9.2 配置更新

每个平台配置使用独立 generation：

1. 校验并保存配置。
2. 在 manager 锁内递增 generation、替换 cancellation token 和内存快照；锁内不做 I/O。
3. 取消旧任务。
4. 在锁外解析凭据并启动新 connector。
5. connector event 携带 generation；manager 丢弃不匹配事件。

### 9.3 退出顺序

在 `prepare_app_exit_inner` 中接入：

1. manager 进入 shutting down，拒绝新 inbound action 和新 outbox claim。
2. 注销/失效 lifecycle subscriber 的 sender。
3. 取消企业微信 connector 及 outbox worker。
4. 等待有界超时，未完成的 `sending` 租约留给下次启动恢复。
5. 继续现有 scheduler、active runtime 和 provider 清理流程。

所有不需用户交互的外部命令仍必须通过 `process::background_command()`；本功能正常实现不应启动任何 helper 进程。

### 9.4 背压

- lifecycle 到 service 使用有界 MPSC，建议容量 256。
- 满载时不在 publisher 上等待；写结构化告警并触发一次局部桌面提醒。由于事件本身可从当前 Runtime 状态重建投递，service 应提供按单个 locator 补投入口，不扫描所有历史。
- connector inbound channel 有界，建议容量 128；平台回调先做最小协议确认，再排队领域处理。
- 每 channel 出站并发默认 1，后续只有在真实吞吐数据证明不足时调整。

## 10. 平台实现

### 10.1 企业微信 Connector

协议基线：`wss://openws.work.weixin.qq.com`。

实现项：

- Bot ID/Secret 鉴权和 WebSocket 建连。
- 默认 30 秒 heartbeat、读写超时、带 jitter 的有界指数退避。
- 会话首次交互建立 `ImBinding`；无 binding 时状态为 `binding_required`，不猜测收件人。
- template card 创建、button/vote callback、卡片终态 update。
- callback 在 5 秒内完成协议确认；Runtime 提交在有界队列异步执行。
- 解析平台事件 ID、操作者和会话 ID，校验绑定后生成 `ImInboundEnvelope`。
- 单 Bot 连接冲突转换为明确错误码，设置页提示用户确保仅一个客户端连接。

接口测试使用录制后脱敏的 JSON fixture 覆盖鉴权、心跳、卡片 button/vote、重复 callback、限流、断线和旧 generation。

## 11. Lifecycle 投影与消息内容

`RemoteInterventionService` 订阅 `InterventionRequested` 后：

1. 从事件取得稳定 identity 和完整 locator。
2. 通过领域 read API 加载当前请求快照、revision、allowed actions 和 expiry；事件不是最终写入依据。
3. 为每个 enabled + bound channel 生成确定性 delivery ID。
4. 构造领域中立 `InterventionPresentation`。
5. 幂等写入 outbox，唤醒对应 channel worker。

Run 与 ACP turn 信息通知复用同一 outbox transport，但使用 `Information` payload。Run/ACP projection 遇到 `scheduled_occurrence_id` 必须跳过；定时任务 completion、failure、attention 和 missed 不产生 IM delivery。Windows notification DTO、dismiss 状态和 `NotificationDedup` 不得作为 IM 输入。

消息只包含用户决策所需内容：请求类型、任务标题、节点标签、最小请求摘要、允许动作、到期时间。禁止包含本地绝对路径、隐藏 prompt、完整上下文、Secret、原始 provider payload 或调试协议字段。

领域请求解决后，service 通过现有 lifecycle 终态或 command result 更新已绑定卡片/发送文本确认。若无法更新原消息，只发送一次幂等终态通知。

## 12. Tauri 与前端 API

### 12.1 Commands

```text
get_im_settings() -> ImSettingsVm
start_wecom_scan_authorization(session_id) -> WeComScanAuthorizationVm
complete_wecom_scan_authorization(session_id) -> ImSettingsVm
cancel_wecom_scan_authorization(session_id) -> ()
set_im_channel_enabled(kind, enabled) -> ImSettingsVm
save_im_notification_preferences(kind, notifications) -> ImSettingsVm
reset_im_channel_binding(kind, expected_generation) -> ImSettingsVm
reconnect_im_channel(kind, expected_generation) -> ImChannelSnapshotVm
delete_im_channel(kind) -> ImSettingsVm
```

Typed events：

```text
im-channel-state-updated
```

所有 command 使用统一 `CommandErrorVm { code, details }` 边界。

### 12.2 设置页 UI

- 在设置页新增“集成”区域，不新增主导航入口。
- 企业微信行显示图标、名称、状态、绑定摘要和菜单操作，不使用嵌套卡片。
- 配置表单复用 shadcn/ui `Dialog`/`Sheet`、`Input`、`Switch`、`Button`、`Alert`，选择器复用项目现有 shadcn Select。
- 企业微信只显示扫码接入/重新授权，二维码 URL 与倒计时只存在于 Dialog 局部状态；关闭 Dialog、重新生成或应用退出都会取消后端活动会话。
- 企业微信 Bot ID/Secret 手工入口删除；授权成功后后端直接把 Secret 写入 OS credential store，Tauri response 和前端状态永不包含 Secret。
- 企业微信展示配置说明链接；不展示微信占位入口。
- 状态包括：未接入、等待绑定、连接中、正在重连、可用、已暂停、需要重新授权、连接被占用。
- 不提供测试连接和独立断开入口；总开关即时启停，冲突恢复使用带 expected generation 的一次真实重连。
- 中英文 UI 与错误码映射同步加入 `web/src/i18n.ts`。

前端 state 以 channel kind 归一化，只订阅企业微信 snapshot。不得把连接 tick、heartbeat 或原始平台事件放进 React state。

#### 12.2.1 清晰度优化交互原型（2026-09-04）

- 已新增单文件交互原型：`docs/gold-band/产品设计文档/interaction/app/原型/IM干预设置界面/code.html`。
- 原型覆盖未接入、等待绑定、连接中、正在重连、可用、已暂停、需要重新授权和连接被占用八种派生态，以及扫码授权、私聊绑定、即时启停、通知 dirty 保存/放弃、重新授权、真实重连、更换接收账号、删除确认、键盘页签和明暗主题。
- 生产实施时复用现有 shadcn/ui Badge、Alert、Switch、Checkbox、Dialog、AlertDialog、DropdownMenu、Separator/Collapsible；不把原型内的原生控件实现复制进生产代码。
- credential、enabled、connection generation、binding 与六字段 notifications 继续是唯一事实来源；生产接口按各自数据所有权收窄，删除通用保存与重复断开命令。原型状态控制器和模拟按钮不得进入产品。
- 对客文案候选：`IM 远程干预与通知` 收敛为 `远程干预`，`Run 成功/失败` 收敛为 `工作流成功/失败`，`ACP 回合结束` 收敛为 `Agent 回复完成`，删除“安装级目标”等实现表达。中英文文案须在生产迁移时同步。
- 已迁移 `ImIntegrationSettings.tsx`：显示模型为纯投影，总开关即时执行；通知草稿下沉到独立表单，不被 snapshot、扫码或启停响应重置；扫码 Dialog 在授权后进入可关闭的等待绑定态，只有持久 binding 与当前 snapshot 一致才完成。
- `BindingObserved` 已改为先校验、再落盘并重建 target、最后提交 snapshot；IM settings 写入通过同一最小临界区与 generation 推进避免旧事件交错。`IM_STORAGE_UNAVAILABLE` 使用单 channel+generation、最多三次的 1/2/4 秒可取消重试。
- 原型接口回归位于 `scripts/im-settings-prototype.test.mjs`，固定渐进披露、扫码到私聊绑定、即时启停、通知保存/放弃、分类恢复、更换账号、删除确认和无障碍语义。
- 性能预算：继续使用当前单 channel 增量订阅；六项通知渲染为 O(1)，不新增轮询、全量设置刷新或无界状态。持久化重试为单任务、有限次数且 generation 变化即取消。过度设计复核确认不增加向导状态机、通知预设、测试连接状态或第二套持久模型。

## 13. 结构化错误码

Rust 定义 typed `ImErrorCode`，Tauri 和 connector 只传 code、retryable 与脱敏 details。至少包含：

| Code | 场景 | 自动重试 |
|---|---|---|
| `IM_CONFIG_INVALID` | 配置字段非法 | 否 |
| `IM_CREDENTIAL_UNAVAILABLE` | keyring 读取/写入失败 | 否 |
| `IM_AUTHENTICATION_REQUIRED` | token 失效或需重新扫码 | 否 |
| `IM_CONNECTION_UNAVAILABLE` | 短暂网络错误 | 是 |
| `IM_PLATFORM_RATE_LIMITED` | 平台限流 | 按 retry time |
| `IM_BINDING_REQUIRED` | 没有可投递会话 | 否 |
| `IM_ACTOR_FORBIDDEN` | actor/chat 不在绑定范围 | 否 |
| `IM_ACTION_INVALID` | 命令或按钮 payload 非法 | 否 |
| `IM_ACTION_EXPIRED` | 请求或 token 到期 | 否 |
| `IM_ACTION_ALREADY_HANDLED` | 请求已有终态 | 否 |
| `IM_REVISION_CONFLICT` | Runtime revision 已变化 | 否 |
| `IM_RUNTIME_STATE_MISMATCH` | 请求类型/owner/状态不符 | 否 |
| `IM_QUEUE_CAPACITY_EXCEEDED` | 有界队列满 | 条件恢复 |
| `IM_PLATFORM_PROTOCOL_ERROR` | 不支持或错误的协议响应 | 视 details |

后端不得包含中文或英文对客句子；前端与出站消息 renderer 分别从 i18n catalog 映射。

## 14. 分阶段实施

### Phase 0：依赖与官方协议 PoC

- [ ] 用隔离样例验证企业微信连接、卡片发送/回调/update。
- [x] 新增隔离 `wecom-vote-poc` runner：复用本机已配置企微凭据与私聊绑定，向手机端/PC 端各发送一张 `vote_interaction` 卡，捕获脱敏回调 JSON，并使用原回调 `req_id` 回写仍为禁用 vote 卡的更新帧。
- [x] 真实手机端与 PC 端分别选择不同 `option.id` 并提交；回调均返回唯一 `selected_items.selected_item[].option_ids.option_id[]`，可还原选中 option，生产 Permission vote 卡获准实施。
- [x] 完成候选 crate 源码、许可证、维护状态、Rust 1.95 和 TLS/runtime 审计。
- [x] 确认微信延期，未把 `weixin-agent` 或官方 npm 包加入生产依赖。
- [x] 把依赖决定和精确版本更新到本计划，不把 PoC 代码直接带入生产模块。
- [x] 核对企业微信官方 CLI 扫码协议、Halo 参考实现与实际 `generate` 响应；确认 `source=halo` 可生成短期二维码，但未宣称跨企业真实授权通过。

退出条件：企业微信有明确 SDK/传输实现决策；无法满足主动投递的平台能力在产品文档中降级，不通过非官方逆向方案补齐。

### Phase 1：统一领域命令服务

- [x] 新增 `InterventionCommandService` 与 typed command/result/error。
- [x] 三个现有 Tauri command 改为委托服务。
- [x] 完整 locator、revision、owner、type 与 manual check 状态校验统一。
- [x] 添加并发、重复、历史 attempt、复合节点接口测试。
- [x] 修正 permission JSON-RPC request identity 与 timeline item identity 混用；接口测试固定 `requestId=0`、`timeline id=permission-0` 仍能直达同一 pending request。
- [x] 从 `PendingElicitationState.request.requestedSchema` typed 投影 scalar `oneOf/enum`、array `items.anyOf/oneOf/enum`、自由文本与 custom-answer companion；多问题/多选完整展示但不提交残缺答案，单题单选继续提交 schema-shaped content，schema 变化触发 expected-state conflict。
- [x] 将远程 Elicitation 契约升级为 channel-agnostic `RemoteElicitationForm`：SingleScalarChoice、MultiScalarChoice、ScalarChoiceQuestions，挂入 typed allowed action 与 expected state；新增泛型 Form selection，回调从 outbox 完整 scalar value 还原 content，并继续用原始 requestedSchema 编译校验。
- [x] 补齐 Elicitation 桌面/IM 共享应用执行边界：inspect/CAS -> scheduled interaction reclaim -> ElicitationResolved metrics cause -> 共享 execute -> root/dynamic session VM -> session update -> attempt index；AlreadyApplied/RevisionConflict 仍 emit 最新投影并收敛为已处理。

退出条件：不接 IM 也能证明桌面入口行为不回归，两个来源共享同一领域不变量。

### Phase 2：IM core、存储和凭据

- [x] 建立公共模型、connector trait、connection manager。
- [x] 创建 `core.db` component schema、repository、租约恢复和 retention。
- [ ] 接入 keyring，完成凭据写入/替换/删除回滚测试（实现与失败回滚已完成，仍缺各目标 OS 的真实 credential store 验收）。
- [x] SettingsConfig 升 v11 并完成 migration/interface tests。
- [x] lifecycle subscriber 完成有界 outbox projection。
- [x] lifecycle projection 补齐每 job completion：blocking/repository 错误使用稳定码可观测，成功持久化后才唤醒 worker，单条失败后 consumer 继续处理下一事件。
- [x] canonical lifecycle event ID 补齐 `task_id`，固定不同 task 复用本地 `run-001` 时产生独立 delivery；Direct Run completed 后的 ACP pending permission/elicitation 继续按当前 owner 与 request identity 可操作，manual check 仍拒绝 completed Run。

退出条件：使用 fake connector 可验证入队、发送、失败重试、重启恢复、容量、过期、幂等和 generation。

### Phase 3：企业微信正式接入

- [x] 实现 connector、binding、卡片与回调 fixture。
- [x] 接入设置 API/UI 和 typed state event。
- [x] 用有界可取消的扫码会话替换 Bot ID/Secret 手工入口，后端直接写 keyring；`source` 进入安装级配置。
- [x] 修复单聊事件缺少 `chatid` 时的 actor/conversation identity，并将 `disconnected_event` 映射为非重试连接冲突。
- [x] 修复 ACP permission/`askUserQuestion` lifecycle bridge 依赖 raw identity 字段导致事件缺失；两类 request identity 统一取 canonical `AcpUiEvent.id`，设置页在线未绑定时持续显示“等待绑定”。
- [x] 对齐官方 SDK 的 `aibot_send_msg`：删除未定义的 `chat_type`，按顶层 `errcode` 结算 ACK，不再强制未承诺的 `body.msgid`；ACK 无消息 ID 时只标记 sent，不写伪平台 binding，未知平台数字码仅进入脱敏诊断。
- [x] 将企微干预卡片从多行 Markdown title 收敛为官方 `main_title + horizontal_content_list + interactive control + task_id` 结构；Permission 与 ManualCheck 使用 `vote_interaction`，Elicitation 因真实 3 按钮横排截断仍最多渲染 2 个 button，复用 deterministic delivery ID 作为卡片 task identity。
- [x] 对齐真实模板卡片回调：只接收私聊 `template_card_event`，以 `body.msgid` 做幂等 identity，先解析 `event.template_card_event.event_key` 短引用取得 delivery identity/action index，再由本地 outbox 权威 `allowed_actions` 还原 typed action；显式 `event.template_card_event.task_id == delivery_id`，button 官方回调省略可选 `task_id` 时用 delivery identity 继续执行并用原 `headers.req_id + delivery identity` 发送 `update_template_card`；vote 要求必填 task、合法 submit key、唯一匹配 question 与单个数字 option id；点击响应不依赖发送 ACK 的 message binding，嵌套 `disconnected_event` 进入连接冲突。
- [x] 权限动作保留 ACP `PermissionOption.optionId/name/kind`，vote 选项与安全策略使用 typed `kind` 而不是解析 option ID：覆盖 Codex `allow_once/allow_for_session/cancel`、Claude `allow/allow_always/reject` 与 ExitPlanMode qualifier；企微标准 Permission 按“记住选择、允许一次、拒绝”展示并默认选中第一项，option id 仍是原 allowed action index。展示排序与超容量安全降级分离，超过 20 项时仍只保留安全拒绝/保持计划或引导桌面端；未知 kind、允许类重复且不可区分时不提交模糊授权，未知错误仅记录稳定脱敏错误码并尽力更新 Handled/Expired/Failed 卡片。
- [x] 生产 Permission 出站改为 `vote_interaction`：`checkbox.mode=0`、短数字 `option.id`、`submit_button.key=<delivery_id>:submit`、原 deterministic task id；终态更新保持 vote 卡、禁用 checkbox、原 task/submit key 并标记最终选中项。真实 PoC 同时确认省略 `submit_button` 会返回 42049。
- [x] 二轮真实验收修复：WebSocket 帧先按是否携带 `cmd` 分流，带 `cmd` 的同 `req_id` 事件不再误消费 pending ACK；Permission 中文卡壳固定为“权限审批 / Agent 请求执行命令 / 请选择授权范围后提交”，并把 `rawInput.command + args` 投影为完整命令，横向字段按工具、命令、路径、参数及结构化字段顺序输出。
- [x] 三轮真实验收修复：Permission 投影补齐 `rawInput.cwd`，企微同一 delivery 内先发送 markdown 详情（说明、工具、完整命令、路径、参数），等待详情 ACK 后才发送 vote 卡，卡片 subtitle 指向上一条详情；卡片 ACK 成功才标记 delivery sent。终态更新补齐与 POC 同形的 `main_title.desc`，ACK 失败只输出字段名、字段类型和数字码层级的脱敏诊断。
- [x] 四轮终态置灰修复：真实日志确认 Runtime 已接受并执行权限，但 blocking 边界误把已取走的 WeCom response context 回退为无关占位 context，导致 `update_template_card` 发送前被 WeCom connector 拒绝。修复为成功/业务失败均使用 `process_inbound` 返回的 enriched WeCom context；join 失败才使用进入任务前克隆的原 channel context，并新增回归防止 response context 被替换。
- [x] 五轮并发配对修复：企微 Permission markdown 详情与 vote 卡标题统一携带 4 位 `display_ref`（durable outbox `rowid % 10000`，不足补零），重试与重启后同一 delivery 编号不变，最近 10,000 次 outbox 插入内不重复；该编号只用于用户配对两条消息，不写入 expected state，也不参与 task id、submit key、回调解析或授权语义，完整 deterministic delivery identity 保持不变。接口测试覆盖中英文标题同编号、短编号重试稳定、完整 task id 保持不变，以及缺失 durable 编号时拒绝发送。
- [x] ManualCheck 决策上下文修复：从当前 attempt 的 timeline index 读取最新可见 root `textDelta`，以 4096 字符快照进入 `InterventionPrompt.message`；企微先发送“人工检查（id=xxxx）+ 最后一轮模型输出”markdown，详情 ACK 后发送同一标题的 vote 卡。vote 按“成功、失败”展示并默认选中成功；提交继续走原 allowed action index / expected state / `submit_manual_check`，终态保持禁用 vote 与原 task、submit key。
- [x] ManualCheck 桌面/IM 一致提交修复：共享 `InterventionCommandService::execute` 补齐 prepare -> background commit 分发；桌面 command 与 IM inbound 均收敛到同一个桌面应用执行边界，统一执行 expected state 校验、`ManualCheckSubmissionLease`、scheduled attention resume、conversation callbacks、`submit_manual_check_background` 与 launch ack。IM 不再启动缺少前端回调的 headless 续跑，也不直接调用外层 Tauri command。
- [x] Permission 桌面/IM 一致收尾修复：桌面 command 与 IM inbound 均收敛到同一个 Permission 应用执行边界，统一 scheduled attention resume、PermissionResolved metrics resume cause、共享 `execute` 权限响应写入、ACP session/dynamic session 重建、前端 session update 与 attempt 索引；IM 处理后桌面 pending permission 卡必须立即消失。
- [x] 企微终态重复点击修复：按官方类型复核确认 `submit_button` 不支持 `disable`，终态只保留原 key 并继续禁用 checkbox/select；`im_inbound_actions` 增加 `channel + canonical_event_id + completed_at` 索引，同一干预后续新 msgid 点击直接记录并返回 `ALREADY_APPLIED`，不再次执行 Runtime。桌面先处理且 ACP waiter 已清理 signal 时，通过 durable timeline response/request 恢复为 AlreadyApplied。
- [x] 企微终态标题与 Elicitation 详情修复：标题改为“id=xxxx 已处理：<摘要>”，display_ref 前置且不可截断；Elicitation markdown 详情通过 timeline index 携带最多 4096 字符的当前 root 前序模型输出，并对 message / question description / context 做规范化去重。
- [x] Elicitation 桌面投影收尾修复：桌面与 IM 已共用 inspect、scheduled reclaim、metrics、response 写入、session rebuild/emit 和 attempt index 边界；前端 reconcile 进一步把同 session 更高 generation/revision/seq 的空 `pendingElicitations` 视为权威终态，不再要求有界 events 页必须同时包含 response event。未前进的陈旧 active snapshot 仍保留 live pending，避免加载竞态闪烁。Web 定向测试 61/61、企微 connector 28/28、IM 全量 68/68、桌面 IM runtime 10/10、TypeScript、Web 生产构建、两个 Rust crate check 与 Rust 格式检查通过；内置浏览器 `/chat` 加载正常且控制台无 warning/error，真实企微到桌面联动仍需新 EXE 复测。
- [x] Elicitation 远程范围分类：仅企业微信处理单题 scalar 单选、单题 scalar 多选、2-3 个 scalar 单选题；custom companion 从 IM 表单移除，固定选项对象必须通过原始 schema 校验。其他场景入队前跳过，不产生 outbox、markdown、提示卡、死信或重试。
- [x] Elicitation 纳入 linked-detail 模式：markdown 详情 -> ACK -> vote mode=0/mode=1 或 multiple_interaction -> ACK；markdown 与卡片展示字符串按 32 KiB 上限在构造层截断，不截断 JSON 字节流或 delivery/question/option identity。
- [x] 实现企微 Form 回调解析：校验 msgid、私聊 actor/conversation、delivery/task、submit key、question key、required selector、重复 option 和非法 index；多选空数组仅当原始 schema 允许且平台显式返回空选择。
- [x] Elicitation 终态保持原 card_type、task_id、submit key 与 display_ref，vote/multiple 全部控件禁用并标记最终选择；AlreadyHandled/RevisionConflict 显示已处理，业务失败保持原结构。
- [x] 桌面审批终态回显到 IM：三类桌面 command 在 canonical 提交前捕获原 intervention event identity，成功后通过既有 canonical event 索引、outbox 和 worker 投影 typed 非交互终态确认。企业微信因官方更新接口必须使用 5 秒内 callback `req_id`，桌面来源不伪造原卡更新，改为发送保留原 4 位 `display_ref` 的“已在桌面端处理”确认；pending 原 delivery 与确认 delivery 在同一事务内 supersede/insert，sending/sent 原卡保留，重复投影幂等。桌面终态先于异步原请求投影时先写 deterministic terminal delivery，迟到原请求在 immediate transaction 内检测终态并跳过，避免审批后又发出活卡。IM 来源仍只用 callback 更新原卡，不生成双消息。
- [x] 桌面终态企微发送分流修复：真实 `runtime.log` 与 `core.db` 证明 Elicitation terminal delivery 已持久化，但旧 connector 仅按 `notification_kind=elicitation` 误入只接受待处理 `Intervention` 的 linked-detail 构建，发送前返回 `IM_PROTOCOL_INVALID`，并把单 delivery 错误升级为永久断连。现改为同时依据 payload 生命周期与 kind 分流，`InterventionResolution` 固定发送单条非交互 markdown；构建/校验失败只返回当前 send 请求并继续 WebSocket session。企微 connector 31/31、核心 IM 74/74、两个 Rust crate check、格式与差异检查通过；会话级测试固定桌面 Elicitation 终态实际写出 `aibot_send_msg`、成功 ACK 后不产生 Disconnected，且无效 delivery 在写帧前失败但不终止连接。没有新增状态、依赖、缓存、队列或网络帧；真实机器人收到终态确认仍需新 EXE 复测。
- [ ] 用真实企微手机端与 PC 端完成 `vote mode=1`、`multiple_interaction` 回调形状采样，以及多端点击、并发桌面/IM、5 秒终态更新和重复 msgid 验收；通过前不得宣称真实平台验收完成。
- [ ] 使用真实测试机器人完成三类干预与竞争提交验收。
- [ ] 完成断网、限流、重启、单连接冲突和旧卡片测试。

退出条件：企业微信达到正式可用标准，日志与存储无凭据泄漏。

### Phase 4：全量回归与发布准备

- [ ] 完成 Windows/macOS/Linux keyring 与构建矩阵；未支持平台需在构建期或 UI 明确关闭。
- [ ] 完成企业微信重启、休眠、退出和断网恢复测试。
- [ ] 检查日志、数据库、settings、crash report 和前端 snapshot 无敏感值。
- [x] 更新两类文档、功能点 Todo 和 MVP 计划；发布说明在真实平台验收后补充。

## 15. 测试矩阵

### 15.1 Rust 单元与接口测试

| 范围 | 必测内容 |
|---|---|
| 领域服务 | 三种请求路由、完整 locator、revision CAS、first-writer-wins、manual owner、outer locator |
| action token | 随机性、作用域、签名/引用、过期、一次性消费、删除配置后失效 |
| repository | schema migration、幂等 insert、due index、claim lease、crash recovery、retention、capacity |
| manager | generation、重配、取消、迟到 event、退避上限、auth required 不重试、shutdown timeout |
| service | `Intervention/Information` 隔离、canonical event 重放、不同 event ID 不聚合、scheduled information event 无 IM delivery、已解决/过期过滤、多目标 delivery、queue full、projection completion 在 delivery 持久化后发布且单条错误不终止 consumer |
| inbound | 私聊 binding、actor/conversation 校验、群聊拒绝、重复 event/action ID、Runtime 成功但审计未写的恢复 |
| connector fixtures | 官方主动推送帧不含 `chat_type`、顶层 `errcode` 必填且 ACK 可无 `msgid`、未知平台数字码脱敏；真实 `event.template_card_event` 使用 `body.msgid` 幂等、嵌套短 `event_key` 指向 delivery/action index、可选嵌套 `task_id` 缺失时恢复原卡 identity、显式 task 不匹配拒绝、私聊/actor/task 校验、嵌套断开、去重、回调确认与限流；SDK 1.0.7 的平铺 event 字段形状不作为运行时回调接受 |
| 企微 Permission/ManualCheck 双发 | mock WebSocket 断言详情 ACK 前 100ms 内不发送 vote 卡；详情 ACK 后按序收到 vote 卡，卡片 ACK 的 msgid 才进入 delivery receipt；Permission 详情包含完整命令、说明、路径、参数且不含 `permissionTitle`，ManualCheck 详情包含最新 root 模型输出并与 vote 卡共享 4 位 `display_ref`；终态保持 POC 同形 `main_title.title + desc`，标题保留同编号，checkbox 与 submit 均禁用；Permission/ManualCheck 共享提交必须与桌面路径一致收敛前端投影、node outcome、manual pending 与 Run 续跑/完成 |
| 企微 Elicitation 表单 | 五类范围分类与 unsupported 无 outbox；单题单选无/有 custom、单题多选、2/3 个单选题出站形状正确；custom companion 移除后仍通过原始 schema；markdown/卡片超 32 KiB 时构造层截断且 identity 不变；vote mode=0/1 与 multiple 回调校验 selector、option、required、重复、task、msgid；多选空数组按 schema 允许执行；截断 label 仍提交完整 scalar；终态保持原控件、task/submit key、display_ref 并全部禁用 |

数据库测试必须使用隔离物理 SQLite 文件和生产 repository 入口，不用 HashMap mock 替代事务与唯一约束。并发测试使用 barrier 固定竞争顺序，不能依赖 sleep 猜测。

### 15.2 前端测试

- API request/response DTO 与 Rust serde contract 对齐。
- 企业微信状态行、credential stored/missing、binding required 和 error code i18n。
- 六类通知安静默认值、逐类持久化与中英文文案；接口拒绝已移除的 scheduled 通知偏好字段。
- 企业微信扫码 Dialog 打开/关闭、重新生成、到期、取消、成功与错误状态；Secret 不进入 response/store，保存时禁用、错误聚焦。
- ACP permission 使用接收 JSON-RPC 时固化的 typed `raw.requestId` 做 Runtime pending/response 查找；timeline/lifecycle 使用 Gold Band 从 `requestId + toolCallId` 生成的 `_goldBandPermissionItemId`，provider 缺失 toolCallId 时使用 durable sequence。elicitation 使用 Gold Band 生成并同时写入 pending state/`AcpUiEvent.id` 的 identity。测试必须证明同一 requestId 的不同 toolCall 不被 outbox 聚合、同一 replay 仍幂等，且两者不靠前缀裁剪或 event ID 反查；在线但未收到私聊 binding 时显示“等待绑定”，收到同一 actor 私聊后局部收敛为已连接。
- `askUserQuestion` 卡片包含真实 message、全部问题标题/描述、单选/多选类型和固定选项；fixture 覆盖 scalar `oneOf/enum`、array `items.anyOf/oneOf/enum`、自由文本与 custom-answer companion。企微仅对单题 scalar 单选、单题 scalar 多选、2-3 个 scalar 单选题生成远程表单；unsupported 场景无 outbox 行且不得静默丢字段或生成无内容接受动作。
- typed event 只更新目标 channel，旧 generation 不覆盖新 snapshot。
- 窄宽度下标签、状态和操作不溢出；键盘导航与 screen reader label 完整。

### 15.3 集成与手工验证

- 真实企业微信机器人完成 permission/elicitation/manual check 与逐类信息通知。
- 桌面与 IM 同时操作同一请求，验证 first-writer-wins。
- 平台重复投递、断网重连、客户端重启、系统休眠恢复、凭据撤销、限流。
- 删除配置后旧卡片无效；重新配置新 generation 正常。
- 核对 settings、SQLite、日志、fixture、前端 state 不含 Secret/token/update token 或用户回答正文。
- 关闭客户端后明确不能实时响应，重开只补发仍有效干预。

## 16. 验证命令与验收记录

每个代码 Phase 至少执行：

```text
cargo fmt --all --check
cargo test -p gold-band <im_or_intervention_target>
cargo test -p gold-band-desktop <im_or_intervention_target>
cargo check -p gold-band
cargo check -p gold-band-desktop
npm run web:test -- <im test files>
npm run web:build
```

实际脚本名称以仓库 `package.json` 为准，不能为通过文档命令创建重复脚本。涉及 UI 时启动前端，使用 deep link `/settings` 和 Codex 内置浏览器验证中文/英文、明暗主题及窄屏；涉及真实 EXE 授权或系统凭据库时再使用桌面客户端验证。完成后关闭本次页面、连接和测试进程。

每阶段验收记录需写明：执行命令、通过数量、真实平台账号类型、未执行项及原因、性能影响、过度设计复核。不得只写“测试通过”。

## 17. 性能预算

| 项目 | 预算/约束 |
|---|---|
| lifecycle subscriber | O(1) 转换与 non-blocking bounded enqueue，不等待网络 |
| 活跃连接 | 最多 2 个 WebSocket |
| outbox | active <= 1,000；due batch <= 32 |
| inbound queue | 每 channel <= 128 |
| lifecycle queue | <= 256 |
| retention cleanup | 单批 <= 200 |
| 用户回答 | <= 8 KiB |
| presentation payload | <= 32 KiB |
| 企微 Permission 双发 | 每个当前 delivery 最多 2 个出站帧，仍只有 1 个 outbox/delivery/task identity |
| permission occurrence identity | 当前事件一次 BLAKE3 哈希，输出固定 64 hex；不扫描 timeline/run，不新增缓存 |
| 重连 | exponential backoff + jitter，默认 60 秒封顶 |
| UI 更新 | 单 channel snapshot，禁止 heartbeat 驱动 React render |

实现评审要检查 SQL query plan 命中 due/retention index、锁不跨 await、网络不在 SQLite transaction 中、无 timeline/会话/run 全量扫描。风险未超过预算时不增加缓存、通用任务调度器或 benchmark；超出时先记录数据规模、基线和验收目标再优化。

## 18. 过度设计与风险复核

### 18.1 明确不建设

- Gold Band 云网关、账号系统、消息代理、Kafka、通用 webhook 平台。
- 独立 IM daemon/helper 和进程间协议。
- 第二套 approval/workflow aggregate。
- 自然语言命令、LLM 意图识别和任意 ChatOps。
- 多客户端分布式选主；首期用一个 bot/app 绑定一个客户端的运营约束。

### 18.2 新增机制的必要性

- `InterventionCommandService`：消除 Tauri command 与 IM 的双写路径，保证领域不变量只有一份。
- connection generation：解决重配/重连异步结果乱序，现有 Runtime revision 无法表达外部连接实例身份。
- bounded outbox：客户端/网络短暂离线与平台至少一次语义要求可靠、有界投递；Runtime 状态不能替代平台 delivery receipt。
- inbound action dedup：平台会重复回调，必须避免同一外部事件重复进入领域层。
- keyring：敏感凭据不能安全地存入现有 settings 或 SQLite。
- permission occurrence identity：JSON-RPC request id 只在 provider 连接内稳定，session resume 后可能重置；timeline/lifecycle 幂等需要 Gold Band 生成的稳定发生身份。

这些机制分别对应具体不变量，没有为假设性规模增加通用抽象。

## 19. 文档同步清单

每个实现 PR 必须同步维护：

- [x] 本实施计划的 Phase checklist 和实际依赖版本。
- [x] 产品设计文档中的能力矩阵、限制或用户流程变化。
- [x] `docs/gold-band/产品设计文档/README.md` 导航。
- [x] `docs/gold-band/开发计划/功能点todo列表.md` 状态和说明。
- [x] 如改变 MVP 范围，更新 `docs/gold-band/开发计划/gold-band-mvp-plan.md`。
- [x] 新增/变更的中英文设置与错误文案。

## 20. 2026-08-30 本地验收记录

- 2026-09-04 IM 设置清晰度优化：根因属于既有 durable settings、OS credential、generation runtime snapshot 与通知投影设计正确，但设置命令粒度、binding 持久化提交顺序和前端草稿边界实现不完整。后端删除通用保存/断开命令，拆分启停、通知保存、更换接收账号、真实重连和删除接口；binding 经 generation 校验后按 settings 落盘、DesktopState 更新、target 重建、connection snapshot 发布的顺序提交，存储失败只对同一 channel + generation 做 1/2/4 秒、最多三次且可取消的有界重试。前端按八类可决策状态渐进呈现，凭据存在后始终显示六项通知，启停即时提交，NotificationDraft 只由通知保存/放弃收敛；旧入口、旧 DTO 和消费路径已删除，不提供兼容层或测试连接。验收：`cargo test im:: --lib -j 1` 75/75、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 15/15、`npm run web:test` 242 个文件共 1647/1647、IM 定向 Web 测试 14/14、HTML 原型测试 6/6、`cargo check --lib -j 1`、`cargo check -p gold-band-desktop -j 1`、`cargo fmt --all -- --check`、`git diff --check` 与 `npm run web:build` 通过。内置浏览器 deep link 覆盖未接入、等待绑定、连接中、可用、暂停、网络重连、凭据失效、连接冲突和长 Bot ID；1024px 正常宽度与 640px 窄窗、重新拉宽、中英文、浅色/深色、扫码弹窗、配置菜单、两类确认、键盘开关/焦点及通知草稿跨启停保持均通过，页面无横向溢出或控制台 warning/error。性能复核确认单通道固定六项配置均为 O(1)，没有轮询、全量刷新、无界缓存/队列或网络 await 持锁；过度设计复核确认未新增持久 UI 状态、global store、aggregate、依赖或兼容模型。真实企业微信授权/冲突/重连、Windows/macOS/Linux 凭据库与发布构建矩阵仍需外部验收；生产构建保留既有混合 import 和大 chunk warning，desktop check 保留既有 dead-code warning。

- 2026-09-04 settings v12 启动迁移修复：上次删除四个 scheduled IM 偏好字段后，领域 DTO 已严格拒绝废弃键，但 `settingsSchemaVersion` 仍停留在 v11，导致已有 v11 `settings.json` 在迁移前反序列化失败，桌面端无法启动；根因属于目标设计正确但持久化迁移不完整。当前将 schema 升至 v12，在既有 `SettingsConfig::from_json_value_with_migration` 边界定点删除每个 channel notifications 中的四个废弃键，再由 `load_settings_file` 原子写回；v12 输入仍严格拒绝这些键，不恢复旧字段、兼容读取或消费路径。接口回归固定 v11 文件迁移、六类有效偏好保留、落盘清理、二次加载幂等以及 v12 废弃键拒绝。验收：迁移定向测试 2/2、配置测试 47/47、核心 IM 73/73、`cargo check --lib -j 1`、`cargo check -p gold-band-desktop -j 1`、`cargo fmt --all --check` 与 `git diff --check` 通过；desktop check 仅保留 10 条既有非 IM dead-code warning。使用本机真实 schema 11 设置执行 `cargo run -p gold-band-desktop`，进程成功启动并保持响应，设置被写回 schema 12，原 channel 通知键只剩六项；测试实例随后关闭。迁移只在旧 schema 启动时对已有 channel 数组做一次 `O(C)` 定点处理，不读取 outbox 或历史运行数据；无新增状态、缓存、队列、依赖或热路径开销。过度设计复核确认复用既有版本迁移和原子写入机制，不增加专用 repair 文件或双模型。

- 2026-09-03 定时任务信息通知退出 IM：根因是后续扩展把定时任务完成、失败、需要处理和错过批次四类信息通知混入 IM 偏好与 lifecycle-to-outbox 投影，超出了 IM 远程干预边界；本次从 `ImNotificationKind`、偏好 DTO、设置页、connector 文案和 scheduled Runtime 桥接中完整删除这四类能力，旧字段与枚举值按开发阶段破坏式更新明确拒绝，不增加兼容层。定时任务 canonical lifecycle、Permission/Elicitation/ManualCheck 远程干预、scheduled attention resume、桌面原生系统通知及设置页独立“完成通知”总开关均保留。验收：`cargo test im:: --lib -j 1` 73/73、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 13/13、Web IM settings 9/9、`cargo check --lib -j 1`、`cargo check -p gold-band-desktop -j 1`、Web 生产构建、`cargo fmt --all --check` 与 `git diff --check` 通过；内置浏览器 `/settings` 验证 IM 通知恰好只剩权限请求、追问、人工检查、Run 成功、Run 失败、ACP 回合结束六项且布局无重叠。删除 scheduled owner、missed batch DTO 与投影桥接后，每次 scheduled 信息事件不再产生 IM 入队工作；无新增扫描、I/O、缓存、队列、状态或依赖，性能风险下降。过度设计复核确认直接收窄既有枚举和投影边界，不复制 canonical 数据、不保留废弃入口。

- 2026-09-03 平台范围收缩：首期只保留企业微信。删除第二平台 connector、SDK 依赖、`ImChannelKind` 枚举值、Runtime worker/connection 装配、手工凭据保存 DTO、设置页输入与中英文文案；通用 repository/manager/projection 测试改用企业微信样例，接口回归固定 settings 只返回 `weCom`。不新增兼容层、迁移字段或第二套状态，既有 Runtime canonical state、outbox、幂等与连接 generation 设计保持不变。验收：`cargo test im:: --lib -j 1` 71/71、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 12/12、Web IM settings 9/9、两个 Rust crate check、Web 生产构建、格式检查与 diff 检查通过；源码、锁文件、前端和两类文档的平台关键词扫描零命中。内置浏览器 `/settings` 验证只显示企业微信扫码接入，不再出现第二平台卡片或手工 App 凭据字段；desktop check 仅保留 10 个既有非 IM dead-code warning。删除平台分支减少 connector/worker 和依赖规模，不新增扫描、缓存、队列或状态，性能无新增风险；过度设计复核确认继续复用单一 canonical state、outbox、幂等与 generation 生命周期。

- 2026-09-03 终态重复点击与 Elicitation 详情闭环：根因确认不是首次执行失败，而是企微终态 submit 无法置灰且新点击生成新 `msgid`，旧幂等边界只覆盖 platform event；ACP waiter 清理 signal 后，迟到点击又被误报为 RequestNotFound/RuntimeStateMismatch。实现 `im_inbound_actions(channel_kind, canonical_event_id, completed_at_ms)` 索引与 canonical 成功结果查询，actor/conversation 校验后直接记录 `ALREADY_APPLIED`；Permission/Elicitation 桌面先处理从 durable timeline 恢复终态，Permission 先 inspect/CAS 后 reclaim。终态保留原 card/task/submit key，禁用 checkbox/select 并移除无效 `submit_button.disable`，标题改为 `id=xxxx 已处理：<摘要>`。Elicitation 通过 timeline index 读取 4096 字符有界的 root 可见前序输出，linked detail 改用 question-only 渲染并规范化去重。验收：`cargo test -p gold-band im:: -j 1` 67/67、`cargo test -p gold-band app::intervention --lib -j 1` 17/17、`cargo test -p gold-band-desktop permission_ -j 1` 20/20、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 10/10、`cargo test -p gold-band-desktop elicitation_desktop_boundary_replays_as_already_applied -j 1` 1/1、`cargo check --lib -j 1`、`cargo check -p gold-band-desktop -j 1`、`cargo fmt --all --check` 与 `git diff --check` 通过；desktop 仅输出既有 dead-code warning。真实企微手机端/PC 端重复提交、5 秒终态更新和视觉截断仍待新 EXE 验收，不得宣称平台已完成验收。性能影响是一条索引查询、timeline index 上的有界候选读取和当前消息内常数级去重；无全量 timeline 扫描、缓存、队列、依赖或第二状态机。
- 2026-09-03 桌面审批向 IM 终态回显闭环：根因是桌面与 IM 虽已共享 canonical 审批执行边界，但卡片终态更新只存在于 IM callback 的 `respond_to_action` 路径；桌面端没有平台 response context。官方企微 `updateTemplateCard` 必须使用对应回调 `req_id` 且受 5 秒窗口约束，因此桌面来源不伪造更新，改由 durable outbox 发送无操作控件的终态确认。提交前从 pending timeline identity 恢复原 event id；仓储通过 `idx_im_outbox_canonical_event` 定位原 delivery，并在一个 immediate transaction 内过期 pending 原卡和幂等插入 `<source>:desktop-resolved`，sending/sent 原卡不改写。终态先到时用当前 eligible target 先写 deterministic terminal delivery，迟到的原请求入队检查同一 identity 后返回 Duplicate。确认沿用原 destination、kind 和 `display_ref`；三类桌面入口成功后唤醒对应 worker，IM 来源仍使用 callback 原卡更新。该实现不新增审批状态机、缓存、依赖或历史扫描，每次桌面审批仅按已投递 channel 做常数级索引查询与事务写入。验收：核心 IM 72/72、桌面 IM runtime 12/12、Permission 20/20、ManualCheck 3/3、Elicitation 桌面重放与三类 source identity 定向测试通过；两个 Rust crate check、格式检查、索引 query-plan 断言、Web 生产构建与内置浏览器 `/chat` 加载通过，浏览器控制台无 warning/error。真实企微桌面审批后的主动确认仍需新 EXE 与真实机器人复测。
- 2026-09-03 Elicitation 远程固定表单实施完成：领域侧新增 `RemoteElicitationForm` 与泛型 Form selection，outbox 固化完整 typed scalar value 并继续用原始 `requestedSchema` 校验；企微支持的单题单选、单题多选、2-3 个单选题进入 linked-detail 双帧，unsupported 场景在入队前跳过；vote mode=0/1、multiple 回调经短 selector/option id 还原 typed content，终态保持原卡结构、task/submit key、display_ref，禁用 checkbox/select 且不发送协议不存在的 submit disable 字段；桌面与 IM 共享 Elicitation 应用收尾边界，重复同一答案收敛 AlreadyApplied。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo check -p gold-band-desktop -j 1`、`cargo test app::intervention --lib -j 1` 16/16、`cargo test im:: --lib -j 1` 66/66、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 10/10、`cargo test -p gold-band-desktop elicitation_desktop_boundary_replays_as_already_applied -j 1` 1/1、`git diff --check` 通过；desktop check 仅保留 10 个既有 dead-code warning。真实企微手机端/PC 端 `vote mode=1`、`multiple_interaction` 回调采样、多端点击、并发桌面/IM、5 秒终态更新和重复 msgid 验收仍未完成，不得宣称真实平台验收完成。
- 2026-09-02 Permission 残留卡片与终态控件根因闭环：代码审计确认 IM 与桌面的权限响应写入均已走共享 `InterventionCommandService::execute`，差异在桌面 command 额外执行 scheduled resume、PermissionResolved metrics resume cause、ACP session/dynamic session 重建、前端 session update 与 attempt 索引，IM 只写 response 导致前端 pending permission 投影残留；回调上下文缺少 durable `display_ref` 导致标题丢失编号。实现 Permission 桌面/IM 共享收尾边界；此前曾按错误协议假设写入 `submit_button.disable=true` 并使用尾置编号，2026-09-03 已修正为移除无效字段、编号前置和 canonical event 幂等。真实企微仍需重启新 EXE 后验证权限卡立即消失、标题保留编号与重复点击不再执行 Runtime。

- 2026-09-02 ManualCheck 审批后不续跑根因闭环：共享 `InterventionCommandService::execute` 对 ManualCheck 直接返回状态错误，而桌面 command 另行执行 prepare/resume/commit，导致企微 inbound 在进入 Runtime 前被拒绝，Run 保持 Paused/AwaitingManualCheck。实现共享 execute 的 prepare -> background commit 分发，并把桌面按钮与 IM inbound 收敛到同一个 `execute_manual_check_intervention` 边界；该边界统一 expected state 校验、`ManualCheckSubmissionLease`、scheduled attention resume、conversation callbacks、`submit_manual_check_background` 与 launch ack，避免 IM 启动缺少前端回调的 headless 续跑。新增回归证明共享 execute 提交后返回 Accepted、清除 `manual_check_pending`、写入 success outcome 并按 `$end` 完成 Run。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo check -p gold-band-desktop -j 1`、`cargo test app::intervention --lib -j 1` 15/15、`cargo test im:: --lib -j 1` 61/61、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 9/9 通过；测试使用 `CARGO_PROFILE_TEST_DEBUG=0`。性能影响为一次 action 分支和现有后台线程/launch ack 复用，无新增扫描、队列、锁或状态机；真实企微仍需重启新 EXE 后验证审批提交、客户端事件、下一节点启动与终态置灰。

- 2026-09-02 ManualCheck 决策上下文根因闭环：根因属于 Runtime 正确设计下的 presentation 上下文不完整，`manual_check_pending`、expected state、allowed actions 与 `submit_manual_check` 已能表达唯一人工判定，但 snapshot 缺少用户要审阅的最后一轮模型输出，企微也仍用两个横排按钮。实现 timeline index 最新可见 root `textDelta` 读取（跳过空、hidden、thought 与嵌套 Agent 分支），4096 字符快照进入 `InterventionPrompt.message` 但不参与 CAS；企微复用 Permission 的“markdown 详情 -> ACK -> vote 卡 -> ACK”契约，两条标题同为 4 位 `display_ref`，`ManualFailure` 默认选中，提交仍由 outbox allowed action index 还原 typed outcome。回归覆盖输出选择、outbox presentation、中英文详情/vote 配对与终态选中项。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo test im:: --lib -j 1` 61/61、`cargo test latest_root_agent_output_skips_empty_hidden_and_nested_text --lib -j 1` 1/1、`cargo test app::intervention --lib -j 1` 14/14 通过；测试使用 `CARGO_PROFILE_TEST_DEBUG=0`。性能影响为每次 ManualCheck 投影一次 index locator 过滤加有界事件读取，无完整 timeline 加载、缓存、队列或新状态机；真实企微仍需重启新 EXE 后验收两条消息到达、提交执行与终态置灰。

- 2026-09-02 权限通知偶发不达根因闭环：真实 raw 帧证明 `session/resume` 后 provider 将 JSON-RPC permission id 重置为 `0`，同一 attempt 中 `Write kelvinzhou.txt` 与 `Write weiqi.txt` 的 toolCallId 不同，却共同投影为 `permission-0`，IM outbox 按语义唯一约束判定 replay 并跳过；下一个请求 id 变为 `1` 后又能接收。根因属于 canonical identity 实现把 transport request id 误用作 permission occurrence identity，而非企微网络偶发失败。实现 Gold Band 生成的 `_goldBandPermissionItemId`：优先由原始 `requestId + toolCallId` 哈希，缺失 toolCallId 时用 durable sequence；timeline、lifecycle 与 outbox 复用该 occurrence identity，pending/response 与 provider response 继续使用原始 requestId。新增回归覆盖同一 requestId 不同 toolCallId、无 toolCallId 的 sequence fallback、provider 伪造内部 item id 被覆盖、outbox 不聚合新发生以及 replay 幂等。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo test permission_ --lib -j 1` 37/37、`cargo test im:: --lib -j 1` 58/58、`cargo test app::intervention --lib -j 1` 14/14、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 9/9、`git diff --check` 均通过；测试使用 `CARGO_PROFILE_TEST_DEBUG=0`，desktop 仍有 3 个既有 dead-code warning，`git diff --check` 仅输出既有 LF/CRLF 转换提示。性能影响为当前权限事件一次固定输出 BLAKE3 哈希与 O(1) 字段写入，无全量扫描、缓存、队列或新状态机；过度设计复核确认未新增第二套审批状态，仅显式化 timeline/lifecycle occurrence identity。真实企微需重启新 EXE 后重复触发“同 attempt、resume 后 requestId 重置”的权限场景，确认两张详情/vote 卡均到达且可独立处理。

- 2026-09-02 企微终态未置灰根因闭环：真实 outbox/inbound 记录证明 `allow-once` 已提交且 Runtime 执行成功，日志同时出现 `IM_PROTOCOL_INVALID platform_code=None`；代码审计确认 `process_inbound` 取走并 enrich WeCom context 后，调用方从空槽读取并 fallback 为无关占位 context，WeCom connector 因此在发送更新帧前拒绝。修复 blocking 边界 context 交接，业务失败也保留 enriched context，join 失败保留原 channel context；新增“成功/业务失败保留 processed WeCom context”和“join 失败保留原 channel context”回归。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo test im:: --lib -j 1` 54/54、`cargo test app::intervention --lib -j 1` 14/14、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 9/9、`cargo test -p gold-band --bin wecom-vote-poc -j 1` 4/4、`git diff --check` 均通过；desktop 测试仍输出 3 个与本轮无关的既有 dead-code warning，`git diff --check` 仅提示既有 LF/CRLF 转换。此前 `main_title.desc/submit_button` 修复仍保留，但它们不是本次剩余故障的阻塞点；真实企微手机端/PC 端置灰验收仍待重启新 EXE 后完成。

- 2026-09-01 企微三轮真实反馈修复：根因确认详情缺失来自横向字段 26 字建议与 `cwd` 投影缺口，终态未置灰来自生产 update ACK 被拒绝且终态帧与 POC 不同形。实现同一 delivery 的“markdown 详情 -> ACK -> vote 卡 -> ACK”顺序发送，卡片 subtitle 关联上一条详情；`rawInput.cwd` 进入 `permissionPath`；终态补齐 `main_title.desc` 并新增只含字段名/字段类型/数字码层级的脱敏 ACK 诊断；移除 core 中 4 个既有 dead-code 函数。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo test im:: --lib -j 1` 54/54、`cargo test app::intervention --lib -j 1` 14/14、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 7/7、`cargo test -p gold-band --bin wecom-vote-poc -j 1` 4/4、`git diff --check` 通过。desktop 测试仍输出 3 个与本轮无关的既有 dead-code warning；真实企微手机端/PC 端需重启新 EXE 后验收两条消息顺序、完整命令/路径展示、提交置灰与 Agent 执行。

- 2026-09-01 企微 Permission `vote_interaction` 生产实施：真实 PoC 闸门已通过，手机端与 PC 端分别提交 `option.id="0"` 和 `"1"`，回调均返回嵌套 `selected_items.selected_item[].option_ids.option_id[]`，可唯一还原本地 action index；省略终态 `submit_button` 会返回 42049，因此更新帧保留原 task/submit key 并禁用 checkbox。生产链路继续使用 pending permission、完整 expected state、outbox allowed actions、原始 optionId 与 `InterventionCommandService`，只修改企微 Permission 投影和回调解析；未知 kind、允许类重复、超过 20 个选项、多选、非法 index、task 不一致、重复 msgid 与 expected-state 冲突均有稳定路径，重复回调不重复执行。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo test im:: --lib -j 1` 52/52、`cargo test app::intervention --lib -j 1` 13/13、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 7/7、`cargo test -p gold-band --bin wecom-vote-poc -j 1` 4/4、`git diff --check` 通过；仅保留既有 dead-code warning 与 Git 行尾提示。性能仍为当前 delivery 的一次 `O(P + F + A)` 投影与 `A <= 20` 的有界排序，回调一次 indexed delivery 读取加短 id 解析，终态 transient 克隆不超过 20 项，无全量扫描、缓存、队列或新状态机。生产 EXE 重启后触发新权限事件，并在手机端/PC 端验证全部选项、提交执行与 5 秒内终态更新仍待完成。

- 2026-09-01 企微 Permission `vote_interaction` 候选方案：根因复核确认为 canonical permission/outbox/InterventionCommandService 设计正确，缺口仅在企微 presentation adapter 无法安全展示 3 个以上按钮。新增隔离 PoC runner 复用现有凭据与绑定，不进入生产模块；出站卡使用 `checkbox.mode=0`、短 option id、`submit_button` 与 deterministic task id，回调证据落盘前脱敏身份字段，更新帧复用回调 `req_id` 并保持禁用 vote 卡。硬性闸门未通过前不改生产 connector/inbound，也不用默认选项或文案推断授权；手机端与 PC 端不同 option 的真实回调采样仍未完成。

- 2026-09-01 企微二次真实点击复核：嵌套回调修复已生效，点击进入 Runtime 后统一被 `INTERVENTION_REVISION_CONFLICT` 拒绝。真实 pending 文件与 outbox 指纹对照确认写入时序为“先落 pending、IM 先 inspect/outbox 固化 expected state、ACP 后回写 `timelineIdentity`”；旧实现把完整 pending 文件纳入指纹，误把展示投影 revision 当成决策状态变化。修复为 permission/elicitation 指纹只序列化决策语义，保留 params/schema 变化冲突与 first-writer-wins，不回改 outbox。同链路的企微卡片更新返回平台码 42045，官方更新模板卡示例确认必须保持 `button_interaction` 并携带 `button_list + task_id`，修复不得把原可点击卡降级为 `text_notice`。新增接口回归固定“inspect 后绑定 timeline identity，旧 snapshot 仍可提交”与更新卡形状；真实企微仍需重启新构建并触发新权限事件验证执行与 5 秒更新。

- 2026-09-01 企微真实回调嵌套结构修复：真实日志证明 8 次点击均已通过 WebSocket 到达客户端，且统一在 connector 入口被 `EVENT_KEY_MISSING` 拒绝；复核官方 SDK issue #22 的真实回调样本后确认业务字段位于 `body.event.template_card_event.event_key/task_id`，SDK 1.0.7 类型定义中的平铺形状与运行时协议不一致。根因属于协议 adapter 依据错误类型形状实现解析，而不是缺少回调配置、按钮 key 过长或 Runtime 未注册；前一条记录中“平台丢弃 event_key”的判断由本条修正。修复为只按真实嵌套契约读取 action 与原卡更新上下文，新增接口测试固定 `button_interaction` 嵌套 fixture，并断言 SDK 平铺形状不再被接受；不新增双路径兼容、回调状态或第二套 action 映射。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo test im:: --lib -j 1` 47/47、`cargo test app::intervention --lib -j 1` 9/9、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 6/6、`git diff --check` 通过，仅保留既有 dead-code warning。性能影响是每次回调多下钻一个 JSON 对象的常数级开销，无新增状态、缓存、扫描或队列。真实企微仍需重启客户端并触发新权限事件，验证按钮执行与 5 秒内卡片更新。

- 2026-09-01 企微按钮截断与真实点击复核：官方 SDK 1.0.7 确认 `button_list` 最多 6 个、button 文案建议 10 字、key 上限 1024 字节，且没有按钮排列字段；真实 3 按钮横排把文案挤压成约两个字，因此不是 2 字符协议上限，而是平台布局约束。真实日志证明点击回调已到达但 `event_key` 缺失，旧实现把完整 action JSON 与 HMAC token 放入 key，被判定为连接器边界设计缺陷。修复为企微最多渲染 2 个 typed 优先动作（`reject_once`、`allow_once`），按钮 key 只携带 `delivery_id:action_index`，inbound 从 outbox 权威 `allowed_actions` 按索引还原动作并继续执行 actor/conversation/expiry/idempotency 校验；不新增映射缓存或第二套状态。验收：`cargo fmt --all --check`、`cargo test im:: --lib -j 1` 46/46、`cargo test app::intervention --lib -j 1` 9/9、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 6/6 通过，仅保留既有 dead-code warning。按钮排序发生在当前请求的有界动作数上并最多渲染 2 个，回调短 key 避免 action JSON 反序列化；无全量扫描或新增缓存。真实企微仍需重启客户端并触发新权限事件验证按钮完整显示、点击执行与 5 秒内卡片更新。

- 2026-08-31 permission 主标题、typed 动作与真实回调修复：真实日志证明点击已到达客户端但被 `TASK_ID_MISSING` 拒绝；复核官方 SDK `TemplateCardEventData.task_id` 为可选字段，根因属协议 adapter 实现过严而非卡片设计缺陷。权限投影保留 ACP `optionId/name/kind`，企微主标题使用权限标题，描述进入 `main_title.desc` 与 `sub_title_text`，按钮按 typed kind 渲染短文案；Codex `allow_once/allow_for_session/cancel`、Claude `allow/allow_always/reject` 与 Claude ExitPlanMode `auto/acceptEdits/bypassPermissions/default/plan` 均有接口断言。回调先解析 typed `event_key`，显式 task 不匹配仍拒绝，缺失可选 task 时复用原 delivery identity 执行并更新卡片。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo test im:: --lib -j 1` 45/45、`cargo test app::intervention --lib -j 1` 9/9、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 6/6 通过，仅保留既有 dead-code warning。性能仍为当前 pending request 的一次 `O(P + F + A)` 投影与常量级动作映射，不扫描历史状态。真实企微需重启客户端并触发新权限事件验收接收、点击与 5 秒更新，本轮回调样本来自真实日志、Claude/Codex 形状来自官方适配器与本地 raw fixture。

- 2026-08-31 卡片展示/点击修复：真实四问题 `askUserQuestion` schema 已固定为接口 fixture，验证 4 个主问题、全部固定选项、`items.anyOf` 多选、scalar enum 与 custom-answer companion 完整投影，远程只保留拒绝；单题单选仍可提交 schema-shaped content。企微官方 SDK 1.0.7 fixture 固定 `cmd/eventtype/body.msgid/chattype/from/event_key/task_id/headers.req_id`，验证 msgid 幂等、task/group 拒绝、原 req_id/task_id 的 `update_template_card` 与嵌套断开。`InterventionCommandService` 8/8、connector 14/14、inbound 4/4、完整 core IM suite 40/40、desktop IM runtime 6/6 与 desktop 单任务 check 通过；action token、actor/conversation 和 callback req_id 的 Debug 输出有专门脱敏回归。1076 项 core 全量回归首次遇到 Windows 页面文件不足（os error 1455）；改用低 debug 单任务编译后 IM 40/40 仍通过，但全量套件未完成：3 个既有 Git 测试串行复跑仍因 Windows 子进程输出非 UTF-8 失败，1 个既有 MCP HTTP 测试收到 502，1 个既有 MCP SSE 测试超过 60 秒未结束；另 2 个全量运行中失败的 Git 测试串行复跑通过。这些结果不记为本轮 IM 回归失败，也不宣称 core 全量通过。真实企业微信接收、点击与 5 秒响应仍未验收。

- 2026-08-31 permission/elicitation 修复：真实日志将 permission projection 失败固定为 `INTERVENTION_REQUEST_NOT_FOUND`，并确认 pending 文件使用 JSON-RPC `0`、lifecycle 曾误用 timeline `permission-0`；修复后 lifecycle 使用结构化 `raw.requestId`。本机真实 pending elicitation fixture 确认 `askUserQuestion` 为 message + 单问题 + 4 个 `oneOf` 选项，现由统一干预服务生成问题正文、选项按钮和 schema-shaped content。企业微信官方 `@wecom/aibot-node-sdk 1.0.7` 类型定义复核了 `sub_title_text` 112 字建议、`button_list` 最多 6 个及 button key 1024 字节边界。Tauri dev watcher 主编译通过并启动新进程；定向 Rust test 链接在 dev 同时运行时因 rustc OOM 未完成，真实新 permission/elicitation 消息接收与按钮回调仍需本轮客户端触发验收，不记录为已通过。

- 2026-08-31 permission 展示与企微回调补齐：真实 pending permission fixture 证明 `params` 内含 `_meta.permission.description`、工具标题、路径与关键参数，`inspect_permission()` 现在结构化投影这些字段并合并任务/节点信息；路径最多展示 3 条并以 `+N` 表达剩余数量，正文和字段按 256 字符有界。权限动作超过企微 6 个按钮上限时完整展示请求但只保留拒绝/取消类动作，禁止截取前 6 个造成部分授权。企微 callback 按官方可选 `chattype` 语义处理：显式 `single/private` 接受，缺失 `chattype` 且无群聊 `chatid` 时按私聊候选，显式群聊或存在 `chatid` 时拒绝；可定位原卡的解析失败会用原 `req_id/task_id` 更新为失败态并输出稳定 `parse_reason`。验收：`cargo fmt --all --check`、`cargo check --lib -j 1`、`cargo test im:: --lib -j 1` 44/44、`cargo test app::intervention --lib -j 1` 9/9、`cargo test -p gold-band-desktop im_runtime::tests -j 1` 6/6 通过；测试使用 `CARGO_PROFILE_TEST_DEBUG=0`，仅保留既有 dead-code warning。性能影响限于当前 pending request 的 `O(P + F + A)` 有界投影，不扫描 timeline/历史 Run；未新增状态、缓存、队列或平台消息兼容层。真实企微接收、点击与 5 秒更新仍需重启客户端后用新事件验收。

- Rust：`cargo test -p gold-band im:: -j 1 -- --nocapture` 通过 36/36；`cargo test -p gold-band wecom -j 1 -- --nocapture` 通过 14/14；`cargo test -p gold-band-desktop im_runtime::tests -j 1 -- --nocapture` 通过 4/4；本次 desktop lifecycle bridge 回归 3/3；`cargo check -p gold-band-desktop -j 1` 通过。Windows 测试链接使用 `CARGO_PROFILE_TEST_DEBUG=0` 与单任务构建后成功，现有非 IM dead-code warning 保留，未为本需求扩大清理范围。
- Web：本次 IM settings 定向回归 10/10，既有 IM/ACP 定向回归 84/84；全量回归 241 files、1642/1642；TypeScript build check 与 Vite production build 通过。生产构建仍报告既有大 chunk 和 opener 静态/动态混用 warning，本次二维码依赖按需动态分包，不进入设置首屏主包或增加首屏网络请求。
- 浏览器：使用 `/settings` deep link 验证中文/英文、浅色/深色、1280x720、760x720 和 560x720、方向键页签切换、加载态、缺少凭据错误态、扫码成功/结构化错误态及 console；二维码 Canvas 可见且非空，Escape 可关闭 Dialog，无 console error。英文页签从 `Personalization` 收敛为 `Appearance`，并减少等分页签内边距，修复 560px 窄窗标签挤压。
- SQLite/性能：repository 测试通过 due/retention `EXPLAIN QUERY PLAN` 索引断言、active capacity、32 条 due batch、200 条 retention batch、租约恢复和 generation；worker 测试固定 SQLite transaction/领域锁在网络 await 前释放。lifecycle publisher 仅做 `O(C)` 有界投影，`C <= 2`，没有 timeline、会话或历史 Run 全量扫描。
- 依赖复核：IM 仅保留企业微信使用的 `tokio-tungstenite 0.30.0`，不再引入第二套平台 SDK、WebSocket、sha1 或 getrandom 版本。`reqwest 0.13.4` 由 Tauri updater 使用，IM 不新增 HTTP/runtime 栈。
- 安全复核：接口测试固定 settings view model 只暴露 `credentialConfigured` 而不暴露凭据引用；企业微信二维码会话响应不包含 `source`、Bot ID 或 Secret，完成响应只新增公开 Bot ID 和凭据存在状态。授权成功后 Secret 由 Rust 直接写入 OS credential store。settings migration、前端 store/snapshot、SQLite schema 和日志扫描未发现 Secret、access/refresh credential 或敏感 update token；设置保存接口不再接受手工 credentials 字段。IM 模块未新增 `Command::new()`，持久凭据与 HMAC signing key 只经 OS credential store。
- 格式检查：`cargo fmt --all --check` 与 `git diff --check` 通过；Git 仅提示工作区既有 LF/CRLF 自动转换策略，不影响检查结果。
- 过度设计复核：没有新增云网关、第二套业务状态、scheduled 聚合器、时间窗口摘要、通用 rate limiter 或兼容入口。新增 service、outbox、generation、credential store 和 inbound dedup 均分别对应跨入口一致性、可靠投递、连接竞态、凭据边界与平台重复回调的已证实不变量。
- 尚未验收：生产 EXE 重启后触发新企微权限事件，并在手机端/PC 端验证全部 vote 选项、提交执行与 5 秒内终态更新；平台限流/重连、系统休眠恢复、Windows/macOS/Linux credential store 与发布构建矩阵也待完成。企业微信 vote PoC 已通过真实账号，但该 PoC 不等同生产 EXE 链路验收；elicitation 文本/form 真实平台表现仍需验证。
- 企微出站故障修复：真实运行记录证明 Run success、ACP turn finished 和 elicitation delivery 均已入 outbox、被 worker claim 后以 `IM_PROTOCOL_INVALID` 死信，排除 publisher、策略、binding 和唤醒链路。对照官方 SDK 1.0.6 后修复多余 `chat_type`、强制 `msgid` 和多行 Markdown card title 三项协议偏差；本地 connector fixture 10/10、ACK 无消息 ID repository 回归 1/1 通过，真实企微重新接收仍待重启客户端后验证，不据 fixture 宣称平台验收完成。
- 重启后 lifecycle-to-outbox 故障修复：旧 delivery 最晚生成于 22:42，故障客户端于 23:52 启动；00:04 RunCompleted 与 00:05 Elicitation 已进入 canonical lifecycle bus，但 `core.db` 无对应 delivery。RunCompleted event ID 缺少 `task_id`，与旧 task 的同名本地 run identity 碰撞；Direct follow-up elicitation 则被“Run completed 一律拒绝”的 owner 校验误伤。修复 canonical identity 生成器与 ACP/Runtime 分域 owner 校验，并补齐 projection consumer 对 blocking JoinError、repository/validation 及 intervention inspect error 的 completion；worker 只在持久化后唤醒，单条失败不终止后续事件。真实企微接收仍需新 canonical event 验证。
- 启动诊断修复：tracing 初始化已前移到 IM runtime、连接配置和 lifecycle subscriber 启动之前，避免启动期 `target_count`、凭据/连接错误及 generation 状态丢失。旧格式 canonical key 测试 fixture 已统一纳入 task scope。该调整不改变 outbox 状态机或投递顺序；真实企微接收仍只以修复后新事件为有效验收样本。
- 本轮回归：canonical notification identity 22/22、`InterventionCommandService` 5/5、IM lifecycle projection 4/4、桌面通知 action/owner 12/12 通过；`cargo check -p gold-band-desktop -j 1` 与 `git diff --check` 通过，仅保留既有 dead-code 与 LF/CRLF 提示。真实企微尚未用修复后新 canonical event 验收，不宣称平台接收通过。
- Connector terminal 契约修复：runtime 不再忽略 `connect()` 结果或 abort 128 容量事件转发器；先排空当前 generation 的事件，再将 terminal error 统一投影为 `Disconnected`。accepted 连接事件只记录 channel、generation、稳定错误码、retryable 与平台数字码，凭据、Bot/目标 identity、payload 和平台文案均不进入日志。
- Connector terminal error 接口回归 1/1 通过，固定 terminal `Disconnected` 保留当前 generation、稳定错误码与 retryable 语义，正常退出不生成伪错误事件。

## 21. 官方参考

- 企业微信智能机器人 WebSocket：https://developer.work.weixin.qq.com/document/path/101463
- 企业微信智能机器人官方 Node SDK：https://github.com/WecomTeam/aibot-node-sdk
- 微信官方包（延期 PoC 参考，不进入首期依赖）：https://www.npmjs.com/package/@tencent-weixin/openclaw-weixin

正式编码前必须重新核对官方文档；外部协议变化以 connector fixture 和 capability 返回值收敛，不侵入领域接口。
