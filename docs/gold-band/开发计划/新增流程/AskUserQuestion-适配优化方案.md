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

### 优化 2：答案文本格式化改进（P1）

#### 问题

当前 `format_elicitation_answer` 将所有答案用 `；` 连接为一行：

```
MySQL；用户认证、日志系统
```

这丢失了问题-答案的对应关系。用户和 AI 都无法分辨哪个值回答的哪个问题。

#### Claude Code 的做法

Claude Code TUI 中，答案通过 `answers` 字典返回，键为问题的完整 `question` 文本。AI 看到的是结构化 JSON，自然知道映射关系：

```json
{
  "answers": {
    "Which database type?": "MySQL",
    "Which features?": "Authentication, Caching"
  }
}
```

但在 Gold Band 的时间线中，答案以用户消息气泡展示，不能是裸 JSON。需要人类可读的格式化文本。

#### 优化方案

**修改 `format_elicitation_answer`，输出逐行格式**：

```rust
// elicitation.rs
pub fn format_elicitation_answer(schema: &Value, content: &Value) -> String {
    let properties = schema.get("properties").and_then(|v| v.as_object());
    let content_obj = content.as_object();

    let mut lines: Vec<String> = Vec::new();

    if let (Some(props), Some(obj)) = (properties, content_obj) {
        for (key, prop_schema) in props {
            let Some(val) = obj.get(key) else { continue };
            let label = prop_schema
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(key);
            let value_str = format_single_value(prop_schema, val);
            lines.push(format!("**{}**：{}", label, value_str));
        }
    }

    if lines.is_empty() {
        // 回退
        return "已选择".to_string();
    }

    lines.join("\n")
}
```

**输出效果**（单问题）：

```
**数据库**：MySQL
```

**输出效果**（多问题）：

```
**数据库**：MySQL
**功能模块**：用户认证、日志系统
**部署平台**：AWS
```

**前端适配**：答案以 `userTextDelta` 发出，配合优化 4（Markdown 渲染），`MessageBubble` 会以粗体标题 + 答案的形式渲染，视觉效果清晰。

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

### 优化 4：用户消息气泡 Markdown 渲染（P2）

#### 问题

elicitation 答案以 `userTextDelta` 事件发出，但 `MessageBubble` 对 `userTextDelta` 走纯文本渲染分支（`event.content`），不经过 Markdown。优化 2 产生的 `**数据库**：MySQL` 格式不会被加粗渲染。

#### Claude Code 的做法

Claude Code 的 TUI 中答案不是人类可见的消息——它直接返回给 AI 处理。但在 Gold Band 的时间线中，答案需要作为用户消息展示。

#### 优化方案

**在 `user_prompt_event` 中增加标记，前端按标记走 Markdown 渲染**：

Rust 侧（`client.rs`，`handle_elicitation_request`）：
```rust
// 改造 user_prompt_event，支持传入额外 raw 字段
// 或者直接构造带 raw 标记的 AcpUiEvent

let mut user_delta = crate::acp::events::user_prompt_event(
    self.seq,
    self.session_id.clone().unwrap_or_default(),
    answer_text,
    None,
    false,
    Vec::new(),
);
// 标记为 elicitation 答案，前端据此走 Markdown 渲染
if let Some(raw) = user_delta.raw.as_mut() {
    raw["elicitationAnswer"] = json!(true);
}
self.persist_event(&user_delta)?;
```

前端侧（`ACPChatDialog.tsx`，`MessageBubble`）：
```tsx
const isUser = event.kind === "userTextDelta";
const isElicitationAnswer = rawObject(event.raw)?.elicitationAnswer === true;

// 渲染内容分支：
{isUser ? (
  isElicitationAnswer ? (
    <Markdown>{event.content ?? ""}</Markdown>  // elicitation 答案走 Markdown
  ) : (
    event.content  // 普通用户输入走纯文本
  )
) : (
  <Markdown>{event.content ?? ""}</Markdown>  // AI 消息走 Markdown
)}
```

**效果**：`**数据库**：MySQL` 渲染为 **数据库**：MySQL。

---

### 优化 5：超时时长可配置（P2）

#### 问题

`ELICITATION_DEFAULT_TIMEOUT` 硬编码为 `Duration::from_secs(300)`（5分钟）。

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

Gold Band 只支持 Claude Code ACP adapter 的 `oneOf`/`anyOf` 格式。MCP 标准使用 `enum`/`enumNames`。

#### Claude Code 的做法

Claude Code 的 ACP adapter 使用 `oneOf`/`anyOf` 格式（非 MCP 标准 `enum`/`enumNames`）。但作为 Gold Band 客户端，应考虑 MCP 标准兼容性。

#### 优化方案

**在前端 `ElicitationCard` 和后端 `format_elicitation_answer` 各增加一个格式分支**：

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

```rust
// elicitation.rs — format_elicitation_answer 中新增
if let (Some(enum_vals), Some(enum_names)) = (
    prop_schema.get("enum").and_then(|v| v.as_array()),
    prop_schema.get("enumNames").and_then(|v| v.as_array()),
) {
    if let Some(s) = val.as_str() {
        if let Some(idx) = enum_vals.iter().position(|v| v.as_str() == Some(s)) {
            let label = enum_names.get(idx).and_then(|v| v.as_str()).unwrap_or(s);
            parts.push(label.to_string());
        }
    }
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
| 4 | P2 | 消息气泡 Markdown 渲染 | TUI 中不展示用户消息 | `events.rs` + `ACPChatDialog.tsx` | 小 |
| 5 | P2 | 超时时长可配置 | TUI 无超时；ACP 模式需要 | `elicitation.rs` + config | 小 |
| 6 | P3 | `enum`/`enumNames` 支持 | ACP adapter 用 `oneOf`/`anyOf` | `ElicitationCard.tsx` + `elicitation.rs` | 小 |
| 7 | P3 | 进度指示器 | TUI 无显式进度 | `ElicitationCard.tsx` | 小 |
| 8 | P3 | 跳过可选问题 | TUI 不支持显式跳过 | `ElicitationCard.tsx` | 小 |

### 实施建议

- **第一轮**（P0 + P1）：完成优化 1、2、3。这三个优化解决了多问题支持 + 确认流程 + 消息可读性，覆盖了核心功能缺口。
- **第二轮**（P2）：完成优化 4、5。提升视觉体验和灵活性。
- **第三轮**（P3）：完成优化 6、7、8。兼容性和完善性增强。
