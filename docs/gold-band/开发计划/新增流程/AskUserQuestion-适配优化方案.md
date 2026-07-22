# AskUserQuestion 适配优化方案

> 对照 Claude Code 原生 TUI 的交互模式，逐项分析 Gold Band 当前的适配差距与优化方案。

---

## 一、Claude Code 原生处理方式参考

### 1.1 设计理念："Seeing Like an Agent"

Anthropic 在 `AskUserQuestion` 工具上经历了三次迭代才找到正确方案：

| 尝试 | 方案 | 失败原因 |
|------|------|---------|
| 1st | 在 ExitPlanTool 上加 `questions` 参数 | 计划和提问语义混在一起，状态无法收敛 |
| 2nd | 修改 Markdown 输出格式，让模型生成格式化文本 | 解析不稳定，模型会"追加额外句子、丢弃选项、或完全放弃结构" |
| **3rd** | **独立的 `AskUserQuestion` 工具** — 结构化 JSON 驱动 | 模型理解工具边界清晰；结构化 schema 防止格式漂移；UI 可渲染阻塞式交互表单 |

### 1.2 Claude Code TUI 的交互模式

在 Claude Code 原生终端 TUI 中，`AskUserQuestion` 以**阻塞式模态框**呈现：

```
┌─────────────────────────────────────────────────────┐
│                                                     │
│  [Format]  How should I format the output?          │  ← header 作为 chip/tag
│                                                     │
│  1. Summary    — Brief overview of key points       │  ← 键盘数字选择
│  2. Detailed   — Full explanation with examples     │
│  3. Other...                                        │  ← 始终可用的自定义输入
│                                                     │
│  Enter your choice (1-3):                           │
│                                                     │
└─────────────────────────────────────────────────────┘
```

**关键特征**：
- **阻塞式**：暂停整个 agent loop，用户回答后从暂停点恢复
- **逐个呈现**：多问题时逐个提问（step-by-step），而非一次性全部展示
- **键盘驱动**：数字选择 + 自由文本
- **"Other" 始终可用**：不依赖选项列表，用户可输入任意文本
- **双向异步通道**：`InputRequest(Question) → UI Layer → InputResponse(Answer)`

### 1.3 多问题的处理方式

当 `AskUserQuestion` 一次提出 2-4 个问题时：

```
第 1 个问题：
┌──────────────────────────────────────────────┐
│  [Tech Stack]  Which tech stack to use?      │
│  1. Next.js + TypeScript                     │
│  2. Python Flask                             │
│  > 2                                         │
└──────────────────────────────────────────────┘
          ↓ 用户选择后，自动进入下一题

第 2 个问题：
┌──────────────────────────────────────────────┐
│  [Core Features]  Which features to enable?  │
│  ☑ 1. Article display                       │
│  ☐ 2. AI integration                        │
│  ☑ 3. Caching                               │
│  [Confirm]                                   │
└──────────────────────────────────────────────┘
          ↓ 用户确认后，全部答案一次性返回
```

---

## 二、逐项优化方案

### 优化 1：多问题向导式逐个展示（P0）

#### 问题

当一个 `elicitation/create` 请求的 `requestedSchema` 有多个 properties（对应 `AskUserQuestion` 的多个 `questions`）时，Gold Band 的 `ElicitationCard` 将它们全部渲染在同一张卡片中。单选字段点击即触发 `onRespond(content)` 提交整个卡片，导致其他字段的答案丢失。

#### Claude Code 的做法

逐个呈现问题。每个问题独立一个"页面"，用户回答后自动进入下一个。所有问题回答完毕后一次性返回完整答案。

#### 优化方案

**改造 `ElicitationCard` 为步骤式组件**：

```
┌─ ElicitationCard ────────────────────────────┐
│                                               │
│  questions: [{ question, header, options }]   │  ← 来自 requestedSchema.properties
│  currentStep: 0                               │  ← 内部状态
│  answers: {}                                  │  ← 逐步积累
│                                               │
│  ┌─ 步骤 1/3 (单选) ──────────────────────┐  │
│  │  [数据库]  请选择数据库类型：            │  │
│  │  ▸ MySQL                                 │  │
│  │    PostgreSQL                            │  │
│  │    Other...                              │  │
│  │               [下一步]                   │  │
│  └──────────────────────────────────────────┘  │
│                                               │
│  ┌─ 步骤 2/3 (多选) ──────────────────────┐  │
│  │  [功能模块]  需要哪些功能模块？          │  │
│  │  ☑ 用户认证                              │  │
│  │  ☐ 日志系统                              │  │
│  │               [确认选择]                 │  │
│  └──────────────────────────────────────────┘  │
│                                               │
│  ┌─ 步骤 3/3 (单选) ──────────────────────┐  │
│  │  [部署平台]  选择部署平台：              │  │
│  │  ▸ AWS                                   │  │
│  │    Vercel                                │  │
│  │               [提交全部答案]             │  │
│  └──────────────────────────────────────────┘  │
│                                               │
└───────────────────────────────────────────────┘
```

**实现要点**：

```typescript
// ElicitationCard 内部状态
const [currentStep, setCurrentStep] = useState(0);
const [answers, setAnswers] = useState<Record<string, unknown>>({});

// fields 仍从 schema.properties 计算，但只渲染当前步骤
const currentField = fields[currentStep];
const isLastStep = currentStep === fields.length - 1;

function handleStepSubmit(value: unknown) {
  const nextAnswers = { ...answers, [currentField.key]: value };
  setAnswers(nextAnswers);
  
  if (isLastStep) {
    onRespond(nextAnswers);  // 最后一步：提交完整答案
  } else {
    setCurrentStep(prev => prev + 1);  // 进入下一步
  }
}
```

**交互细节**：
- 非最后步骤的按钮文案为 `"下一步"`，最后步骤为 `"提交"` 或 `"确认"`
- 支持回退：左上角可显示 `← 返回`（可选，Claude Code TUI 不支持回退）
- 进度指示器：可选 `步骤 2/3` 或圆点 `● ● ○`（P3 专项优化）

---

### 优化 2：答案数据保持结构化（P1）✅

答案继续以 elicitation response 的结构化 `content` 返回 Agent，并持久化到 `elicitationResponse` 事实中。前端不再把它格式化为独立用户消息；用户可见的历史由 `AskUserQuestion` 工具卡片承载，因此无需维护第二套 `format_elicitation_answer` 展示格式。

---

### 优化 3：单选增加选中态 + 确认按钮（P1）

#### 问题

当前单选字段（`oneOf`）点击即提交。多选字段需要勾选后点击"确认"按钮。两种模式行为不一致。而且单选无法反悔——点错就提交了。

#### Claude Code 的做法

Claude Code TUI 是键盘驱动的：用户输入数字选中选项，然后按回车确认。选中态高亮，用户可以改数字再回车。所有问题答完后才一次性返回。

#### 优化方案

**统一单选和多选的操作模式**：

```
单选交互：
┌──────────────────────────────────────────┐
│  [数据库]  请选择数据库类型：              │
│                                          │
│  ● MySQL          ← 选中态（蓝色边框）     │
│    PostgreSQL                              │
│    MongoDB                                │
│    Other...                               │
│                                          │
│                    [确认选择]  ← 点击后提交│
└──────────────────────────────────────────┘
```

**实现要点**：

```typescript
// 单选：新增 selectedValue 状态，替换直接 onRespond
const [selectedValue, setSelectedValue] = useState<string | null>(null);

function handleOptionClick(optionValue: string) {
  setSelectedValue(optionValue);
  // 不立即提交，等待用户点击确认
}

function handleConfirm() {
  if (!selectedValue) return;
  const content = buildContent({ [currentField.key]: selectedValue });
  handleStepSubmit(content);  // 进入下一步或提交最终答案
}
```

**视觉反馈**：
- 选中态：`border-primary bg-primary/5`（与现有多选选中态一致）
- 确认按钮在选中后出现，带 `Check` 图标
- 点击其他选项可切换选中

**与优化的关系**：此优化与 P0（多问题向导式）独立但配合紧密。即使不做 P0，单选增加确认也能改善体验。但配合 P0 时，最后一步的确认按钮文案改为 `"提交全部答案"`。

---

### 优化 4：回答历史展示收敛 ✅

elicitation 答案属于 `AskUserQuestion` 工具交互结果，不是新的用户 prompt。最终方案不再生成 `userTextDelta` 或独立回答气泡：提交后交互卡片消失，`elicitationResponse` 仅用于 answered 状态回放；消息流保留原生 `AskUserQuestion` 的 tool call 卡片、completed 状态与工具输出。这样与 Claude Code TUI 的交互语义一致，也避免同一回答同时以工具输出和用户气泡重复展示。

---

### 优化 5：超时时长可配置（P2）

#### 问题

---

### 优化补充：同会话追问与 runtime 中断统一收敛（P0）

#### 问题

当前存在两类表面相似、但生命周期实现不一致的 elicitation：

1. runtime 执行中的阻塞式 `elicitation/create`
2. 节点成功后，同一 ACP session 内继续追问时触发的 `elicitation/create`

前者在 run pause / stop / app close 时，会由 runtime 中断路径主动写入 declined response，并把 session snapshot 收敛为 cancelled；后者如果用户在卡片出现后直接关闭应用，live waiter 消失，但“已回答 / 已跳过”的 durable replay 事实不一定写进 timeline，导致重进会话后卡片再次弹出。

#### 目标

不要继续把“runtime 执行中提问”和“成功后追问”视为两条不同方案，而是统一到同一个 **ACP attempt 生命周期**：

- 只要 attempt 对应的 ACP session 仍是 active，就属于待收敛对象
- 无论是 runtime 主链、同会话追问还是 AI-DYNAMIC 内层 attempt，关闭应用 / 启动恢复 / stop 时都走同一套 pending interaction 清理
- elicitation 的 answered / skipped 必须能可靠回放，不能只依赖前端内存态或 live waiter

#### 优化方案

1. `respond_elicitation` 写入 durable response signal
   command 提交用户决策时写 `acp.elicitation-response.<id>.json` 并 upsert `elicitationResponse`，但不根据 snapshot/session 状态删除 signal。run 已完成后的 follow-up prompt 仍可能有活跃 waiter，因此是否可清理只能由真正消费信号并完成 JSON-RPC 回包的 runtime 决定。

2. 应用关闭 / 启动恢复统一扫描 active ACP attempts
   现有 `stop_all_running_sessions()` / `recover_interrupted_running_sessions()` 只覆盖 workflow 维度的 running run，不足以覆盖“run 已完成但 follow-up ACP session 仍 active”的场景。需要新增 attempt 级扫描：
   - 遍历 runtime store 中所有 attempt
   - 识别 `acp.snapshot.json` / `acp.session.json` 仍为 active 的 ACP session
   - 统一调用 pending permission / pending elicitation cancel，并把 snapshot 收敛为 cancelled

3. 固定 signal 所有权与清理顺序
   - command 是 response signal producer
   - runtime waiter 是 consumer
   - runtime 读取 signal 后持久化规范 response，发送 JSON-RPC response 成功后清理 request/response
   - stop/close/timeout 负责取消和陈旧信号收敛

#### 验收标准

- 用户在会话提问卡片弹出后直接关闭应用，再打开并重进会话，不会再次看到已回答或已跳过的同一张卡片
- runtime 执行中断与成功后 follow-up 追问，在 pending elicitation 收敛上的最终可观察结果一致
- `acp.timeline.jsonl` 中始终能找到与最终 UI 状态一致的 `elicitationResponse`，且不会额外生成 synthetic user message

`ELICITATION_DEFAULT_TIMEOUT` 当前为 `Duration::MAX`，ACP runtime 默认持续等待直到用户响应或运行被取消。

#### Claude Code 的做法

Claude Code TUI 模式下没有超时概念——终端开着就一直等。但在 headless/ACP 模式下，合理的超时是必要的。

#### 优化方案

**提取为配置项**：

```rust
// config.rs 或现有的 desktop config 结构中新增
pub struct AcpElicitationConfig {
    pub timeout_seconds: u64,  // 默认 300
}

// elicitation.rs 中读取
pub fn elicitation_timeout(config: &AcpConfig) -> Duration {
    Duration::from_secs(config.elicitation_timeout_seconds)
}
```

前端设置页可增加滑块：`30s / 1min / 5min / 10min / 永不超时`。

---

### 优化 6：`enum`/`enumNames` 支持（P3）

#### 问题

Gold Band 当前已支持 Claude Code ACP adapter 的单选 `oneOf` 与多选 `type=array + items.anyOf` 格式。MCP 标准还常见 `enum`/`enumNames`。

#### Claude Code 的做法

Claude Code 的 ACP adapter 单选使用 `oneOf`，多选使用 `type=array + items.anyOf`（都不同于 MCP 标准 `enum`/`enumNames`）。但作为 Gold Band 客户端，应考虑 MCP 标准兼容性。

另外，AskUserQuestion 的 `header/title` 与 `question` 语义不同：`header` 适合做短标题，真正题干应优先展示 `question` 对应的 schema `description` 或请求 `message`，不能只显示简短 header。

#### 优化方案

**在前端 `ElicitationCard` 增加枚举格式分支**；后端直接透传结构化 content，无需维护展示格式化器：

```typescript
// ElicitationCard.tsx — fields 计算中新增枚举处理
if (prop.enum && Array.isArray(prop.enum)) {
  const enumLabels = prop.enumNames ?? prop.enum;
  result.push({
    key,
    isSelect: true,
    isMulti: false,
    title: prop.title,
    description: prop.description,
    options: prop.enum.map((value: string, i: number) => ({
      value,
      label: enumLabels[i] ?? value,
    })),
  });
}
```

---

### 优化 7：进度指示器（P3）

#### 问题

多问题步骤式流程中，用户不知道当前在第几题、还剩几题。

#### Claude Code 的做法

Claude Code TUI 逐个展示问题，没有显式进度指示器。但终端底部显示当前输入提示。

#### 优化方案

**卡片顶部增加轻量进度指示**：

```
━━━ 数据库选择 (2/3) ━━━
┌──────────────────────────────────┐
│                                  │
│  ● ● ○                           │  ← 圆点指示器
│  步骤 2 / 3                      │  ← 或文字
│                                  │
│  [功能模块]  需要哪些功能模块？    │
│  ...                             │
└──────────────────────────────────┘
```

**实现**：

```tsx
// ElicitationCard 顶部
{fields.length > 1 && (
  <div className="flex items-center gap-2 px-1 mb-2">
    {fields.map((_, i) => (
      <span
        key={i}
        className={cn(
          "size-2 rounded-full transition-colors",
          i <= currentStep
            ? "bg-primary"
            : "bg-muted-foreground/20"
        )}
      />
    ))}
    <span className="text-xs text-muted-foreground ml-1">
      步骤 {currentStep + 1}/{fields.length}
    </span>
  </div>
)}
```

---

### 优化 8：跳过可选问题（P3）

#### 问题

当前每个问题都必须回答——无"跳过"按钮。

#### Claude Code 的做法

Claude Code TUI 的 "Other" 选项允许用户输入任意文本（包括空值）。但显式的"跳过"不支持。

#### 优化方案

**依赖 `required` 字段决定是否显示跳过按钮**：

```json
// MCP schema 中
{
  "type": "object",
  "properties": { "db": { ... }, "features": { ... } },
  "required": ["db"]  // 只有 db 是必填的
}
```

```typescript
// ElicitationCard — 非必填字段显示"跳过"按钮
const isRequired = schema.required?.includes(currentField.key);

{!isRequired && (
  <button
    className="text-xs text-muted-foreground hover:text-foreground mt-2"
    onClick={() => handleStepSubmit(null)}
  >
    跳过此问题 →
  </button>
)}
```

---

## 三、优化总结

| # | 优先级 | 优化项 | Claude Code TUI 参考 | Gold Band 改动范围 | 工作量 |
|---|--------|--------|---------------------|-------------------|--------|
| 1 | **P0** | 多问题向导式逐个展示 | 逐个呈现问题，阻塞式模态框 | `ElicitationCard.tsx` | 中 |
| 2 | P1 | 答案文本格式化 | `answers` 字典（结构化 JSON）→ Gold Band 需人类可读 | `elicitation.rs` | 小 |
| 3 | P1 | 单选选中态+确认按钮 | 数字选择 + 回车确认 | `ElicitationCard.tsx` | 小 |
| 4 | ✅ | 回答历史展示收敛 | TUI 中不展示独立用户消息 | 关闭交互卡片并保留 AskUserQuestion 工具卡片 | 已完成 |
| 5 | P2 | 超时时长可配置 | TUI 无超时；ACP 模式需要 | `elicitation.rs` + config | 小 |
| 6 | P3 | `enum`/`enumNames` 支持 | ACP adapter 用 `oneOf`/`anyOf` | `ElicitationCard.tsx` + `elicitation.rs` | 小 |
| 7 | P3 | 进度指示器 | TUI 无显式进度 | `ElicitationCard.tsx` | 小 |
| 8 | P3 | 跳过可选问题 | TUI 不支持显式跳过 | `ElicitationCard.tsx` | 小 |

### 实施建议

- **第一轮**（P0 + P1）：完成优化 1、2、3。这三个优化解决了多问题支持 + 确认流程 + 消息可读性，覆盖了核心功能缺口。
- **第二轮**（P2）：完成优化 4、5。提升视觉体验和灵活性。
- **第三轮**（P3）：完成优化 6、7、8。兼容性和完善性增强。
