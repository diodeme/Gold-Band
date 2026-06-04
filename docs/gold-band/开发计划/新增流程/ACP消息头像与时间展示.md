# ACP 消息头像与时间展示

## 0. 当前实现状态

- **Agent 文本消息**：左侧展示机器人头像，`ACPMessageList` 通过 prompt-kit `Message` 组件渲染。
- **用户消息**：右侧展示用户头像。
- **Tool call 卡片**：通过 prompt-kit `Tool` 组件渲染为结构化卡片，**不展示头像**，仅保留横向位置（`pl-10` 或等价的左侧缩进）。
- **Thought / Thinking 结构化行**：通过 prompt-kit `ChainOfThought` 组件渲染，**不展示头像**，仅保留横向位置。
- **Plan block**：`PlanBlock` 组件渲染，不展示头像。
- **处理中状态**（"思考中 / 工具调用中 / 回复生成中"）：composer 内展示，不作为消息流卡片。
- **结论**：当前仅用户消息和 agent 文本消息展示头像，所有 ACP 结构化行（tool call、thought、plan）均无头像、无时间。

---

## 1. 核心方向

**变更点**：ACP 工具调用行（tool call cards）和 ACP 思考/结构化行（thinking/structured rows）**也需要展示头像**，并在头像下方展示当前消息时间（`HH:mm` 格式）。

**设计意图**：
- 头像让用户快速区分"谁在执行操作"，强化 agent 作为独立对话参与者的心智。
- 时间戳帮助用户在长会话中定位事件发生的时间点，补全 audit trail。
- 头像 + 时间的组合保持与用户消息、agent 文本消息一致的视觉节奏，形成统一的会话时间轴。

---

## 2. 需求规格

### 2.1 头像展示规则（更新后）

| 消息类型 | 头像位置 | 头像内容 |
|---|---|---|
| 用户消息（`userTextDelta`） | 右侧 | 用户头像 |
| Agent 文本消息（`textDelta`） | 左侧 | 机器人/Agent 头像 |
| Tool call 卡片（`ToolCall` / `ToolCallUpdate`） | **左侧**（新增） | 机器人/Agent 头像 |
| Thought / Thinking 结构化行（`ThoughtDelta`） | **左侧**（新增） | 机器人/Agent 头像 |
| Plan block（`Plan`） | **左侧**（新增） | 机器人/Agent 头像 |
| 处理中状态（"思考中 / 工具调用中 / 回复生成中"） | 无头像 | composer 内展示，不进入消息流 |

### 2.2 头像下方时间展示

**位置**：头像正下方，居中对齐。

**展示内容**：

```text
 [头像]
 15:20
```

- 时间格式为 `HH:mm`（24 小时制，如 `15:20`、`09:05`、`17:30`）。
- 使用 `text-[10px] text-muted-foreground/60 leading-none`，与头像居中对齐。
- 时间来源为该条事件的 `timestamp` 字段，前端解析后取时/分部分。
- **所有展示头像的消息类型均显示时间**，包括用户消息、agent 文本消息、tool call 卡片、thought 行、plan block。
- 同一分钟内连续的多条同类型消息（如连续 thought delta 合并为一个 thought block），时间取**第一条**事件的时间戳。

### 2.3 Tool Call 卡片布局

更新后的 tool call 卡片行布局：

```text
[头像]  [Tool Call 卡片内容]
 15:20   ├─ 工具名 / title
         ├─ status 状态
         ├─ input / output 摘要
         └─ 展开详情
```

- 头像与卡片顶部对齐。
- 时间显示在头像正下方。
- 卡片内容区域保持现有 prompt-kit `Tool` 组件的布局（标题左对齐、紧凑高度、折叠/展开）。
- 头像 + 时间的左侧列宽度与其他消息类型一致，保持时间轴对齐。

### 2.4 Thought / Thinking 结构化行布局

更新后的 thought 行布局：

```text
[头像]  [ChainOfThought 折叠组件]
 15:20   └─ 思考耗时（如 "12 秒"）
         └─ 展开后的思考内容
```

- 头像与 thought 组件顶部对齐。
- 时间显示在头像正下方。
- 组件内容区域保持现有 prompt-kit `ChainOfThought` 组件的布局。
- 连续 thought delta 合并后，时间取第一条 delta 的时间戳。

### 2.5 Plan Block 布局

更新后的 plan block 布局：

```text
[头像]  [Plan Block 内容]
 15:20   ├─ plan step title
         ├─ status
         └─ nested entries
```

- 头像与 plan block 顶部对齐。
- 时间显示在头像正下方。

### 2.6 头像资源

- 所有 Agent 侧结构化行使用**同一机器人/Agent 头像**，与 agent 文本消息头像一致。
- 头像资源复用现有 agent 消息头像组件，不新增头像资源。
- 如后续支持多 Agent 协作（子 Agent），子 Agent 结构化行使用对应子 Agent 的头像。

---

## 3. 前端改动

### 3.1 头像 + 时间容器组件

抽取公共的 `AcpAvatarWithTime` 组件：

```tsx
interface AcpAvatarWithTimeProps {
  side: 'left' | 'right';     // 左侧 agent / 右侧 user
  timestamp: string;           // ISO 时间戳
  avatarUrl?: string;          // 头像 URL
  children: React.ReactNode;   // 消息内容（Tool 卡片 / ChainOfThought / Plan / Message）
}

function AcpAvatarWithTime({ side, timestamp, avatarUrl, children }: AcpAvatarWithTimeProps) {
  const time = formatTime(timestamp); // "15:20"
  const avatar = side === 'left'
    ? <AgentAvatar url={avatarUrl} />
    : <UserAvatar />;

  const avatarColumn = (
    <div className="flex flex-col items-center gap-0.5 shrink-0 w-10">
      {avatar}
      <span className="text-[10px] text-muted-foreground/60 leading-none">{time}</span>
    </div>
  );

  return side === 'left' ? (
    <div className="flex items-start gap-3">
      {avatarColumn}
      <div className="flex-1 min-w-0">{children}</div>
    </div>
  ) : (
    <div className="flex items-start gap-3 justify-end">
      <div className="flex-1 min-w-0">{children}</div>
      {avatarColumn}
    </div>
  );
}
```

### 3.2 各消息类型接入

| 组件 | 改动 |
|---|---|
| Agent 文本消息（`textDelta`） | 接入 `AcpAvatarWithTime`，`side="left"`，头像下方补时间 |
| 用户消息（`userTextDelta`） | 接入 `AcpAvatarWithTime`，`side="right"`，头像下方补时间 |
| Tool call 卡片 | 接入 `AcpAvatarWithTime`，`side="left"`，头像 + 时间 + Tool 卡片 |
| Thought 结构化行 | 接入 `AcpAvatarWithTime`，`side="left"`，头像 + 时间 + ChainOfThought |
| Plan block | 接入 `AcpAvatarWithTime`，`side="left"`，头像 + 时间 + PlanBlock |

### 3.3 时间格式化

```typescript
function formatTime(isoTimestamp: string): string {
  const date = new Date(isoTimestamp);
  const hours = date.getHours().toString().padStart(2, '0');
  const minutes = date.getMinutes().toString().padStart(2, '0');
  return `${hours}:${minutes}`;
}
```

- 使用本地时区。
- 24 小时制。
- 无效时间戳时显示 `--:--`，不报错。

### 3.4 移除旧缩进

- 移除当前 tool call / thought / plan 行为了对齐头像而设置的裸 `pl-10` 或等价左侧缩进。
- 统一由 `AcpAvatarWithTime` 的 `gap-3` + `w-10` 控制间距。

### 3.5 涉及文件

| 文件 | 改动 |
|---|---|
| `web/src/components/acp/AcpAvatarWithTime.tsx` | **新建**：头像 + 时间公共容器组件 |
| `web/src/components/acp/ACPMessageList.tsx` | agent 文本消息、用户消息接入 `AcpAvatarWithTime` |
| `web/src/components/acp/ToolCallCard.tsx` | 接入 `AcpAvatarWithTime`，移除旧缩进 |
| `web/src/components/acp/ThoughtBlock.tsx` | 接入 `AcpAvatarWithTime`，移除旧缩进 |
| `web/src/components/acp/PlanBlock.tsx` | 接入 `AcpAvatarWithTime`，移除旧缩进 |
| `web/src/components/acp/ChildAgentGroupCard.tsx` | 子 Agent 分组卡片的 header 行接入 `AcpAvatarWithTime`（如适用） |

---

## 4. 设计约束

- **头像大小**：与现有 agent 消息头像一致（`h-8 w-8` 或当前实际值），不因新增时间展示而缩小。
- **时间字号**：`text-[10px]`，足够小不抢占主内容视觉，但可读。
- **时间颜色**：`text-muted-foreground/60`，进一步弱化，不干扰消息内容。
- **间距**：头像列宽度固定（`w-10`），头像与时间之间 `gap-0.5`（2px），头像列与内容区之间 `gap-3`（12px）。
- **深色主题**：时间颜色在深色主题下使用 `text-muted-foreground/50`，避免过亮。
- **对齐**：头像列垂直方向与内容顶部对齐（`items-start`），不居中对齐。

---

## 5. 本期约束

- 不改变处理中状态（"思考中 / 工具调用中 / 回复生成中"）的展示位置，仍放 composer 内。
- 不改变头像资源本身，复用现有 agent / user 头像。
- 不改变 prompt-kit `Tool`、`ChainOfThought` 等组件的内部布局，只在外部包装头像列。
- 不支持头像点击交互（本期不做头像菜单、 profile 弹窗等）。

---

## 6. 验收标准

- [ ] Tool call 卡片行展示左侧机器人头像，头像下方展示 `HH:mm` 时间。
- [ ] Thought / Thinking 结构化行展示左侧机器人头像，头像下方展示 `HH:mm` 时间。
- [ ] Plan block 展示左侧机器人头像，头像下方展示 `HH:mm` 时间。
- [ ] Agent 文本消息头像下方展示 `HH:mm` 时间。
- [ ] 用户消息头像下方展示 `HH:mm` 时间。
- [ ] 所有消息类型的头像列宽度一致，时间轴视觉对齐。
- [ ] 时间格式为 24 小时制 `HH:mm`（如 `15:20`）。
- [ ] 深色主题下时间文字不过亮，不干扰消息内容阅读。
- [ ] 旧有的裸 `pl-10` 缩进已移除，由头像容器统一控制间距。
- [ ] 流式输出过程中，tool call / thought 行一旦出现即展示头像和时间（不等待完整输出）。
- [ ] 合并后的 thought block 时间取第一条 delta 的时间戳。
- [ ] 无效时间戳时显示 `--:--` 而不报错或空白。
