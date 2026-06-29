# Gold-Band MCP & SKILL 管理 — 完整设计方案（最终版）

> 基于 5 轮深度访谈 + 完整开发实现（对标 Zed）
> 更新：2026-06-26
> 涵盖：MCP 服务管理、MCP 健康检查、SKILL 管理、运行时集成
>
> 说明：runtime prompt 已移除 `skill_catalog` 注入；本文件中关于 `skill_catalog_block.md` 注入 system prompt 的早期设计仅作为历史背景，不再代表当前运行时链路。

---

## 一、架构总览

### 模块结构

```
src/
├── mcp/mod.rs              ← MCP 管理器（对标 Zed ContextServerStore + ContextServer）
├── skill/mod.rs            ← SKILL 管理器（对标 Zed agent_skills + SkillIndex）
├── config/mod.rs           ← 共享数据模型（McpServerState, ToolInfo, SkillMeta 等）
├── storage/mod.rs          ← 路径管理（GoldBandPaths: global/project skills dirs）
├── app/mod.rs              ← 委托层（App → McpManager / SkillManager）
├── app/node_executor.rs    ← 运行时集成（WorkerInvocation 构建）
├── acp/client.rs           ← ACP mcpServers 传递（session/new + session/load）
├── provider/mod.rs         ← System/User Prompt 渲染
├── prompts/
│   ├── {en,zh-CN}/runtime/system.md              ← 稳定 runtime 规则
│   └── {en,zh-CN}/runtime/hidden_context.md      ← 每次 invocation 的 hidden runtime context
└── prompts.rs              ← include_str! 常量
```

### 对标关系

| Gold-Band | Zed | 对齐程度 |
|-----------|-----|----------|
| `McpManager` | `ContextServerStore` + `ContextServer` | ✅ 完整对标 |
| `McpServerState` | `ContextServerState` | ✅ 状态机对齐 |
| `SkillManager` | `agent_skills` + `SkillIndex` | ✅ 完整对标 |
| `apply_skill_overrides()` | `apply_skill_overrides()` | ✅ 同名函数 |
| `select_catalog_skills()` | `select_catalog_skills()` | ✅ 同名函数 |
| `mcpServers` ACP 字段 | `into_new_session_request().mcp_servers()` | ✅ |
| `skill_catalog_block.md` | `system_prompt.hbs` `<available_skills>` | ✅ 模板对齐 |
| ContextManagementPage | Agent Panel Settings | ✅ |
| SkillTool (工具调用) | `SkillTool` (AgentTool trait) | ❌ 架构约束（路径 A 嵌入替代） |
| 斜杠命令 SKILL | Slash Commands | ❌ 架构约束 |
| Worktree Trust | `TrustedWorktrees` | 🔜 后续 PR |

---

## 二、MCP 管理

### 2.1 核心决策

| # | 决策 | 结论 | 对标 Zed |
|---|------|------|----------|
| 1 | 传递方式 | ACP `mcpServers` 字段 + System Prompt `{{mcp_tools}}` 占位符 | ✅ `into_new_session_request()` |
| 2 | 编辑方式 | Local/Remote Tab + JSON 编辑器 | ✅ `ConfigureContextServerModal` |
| 3 | JSON 解析 | 后端 strip `///` 注释 + lenient JSON parse | ✅ `parse_input()` |
| 4 | Server ID | JSON 顶层 key 即 id | ✅ |
| 5 | 传输类型 | stdio + HTTP（含 OAuth 支持） | ✅ |
| 6 | 健康检查 | MCP initialize 握手（Stdio + HTTP 统一协议） | ✅ `server.start()` |
| 7 | 健康门控 | 仅传递 enabled + healthy 的服务器给 ACP | ✅ `maintain_servers` |
| 8 | 状态机 | `McpServerState: Starting → Running{tools} → Stopped/Error/AuthRequired` | ✅ `ContextServerState` |
| 9 | 状态缓存 | `RefCell<HashMap<String, McpServerState>>` 内存缓存 | ✅ |
| 10 | 保存策略 | 先保存 → 再验证（"先存后验"） | ✅ |
| 11 | enabled 开关 | 独立于健康状态，始终可手动切换 | ✅ |
| 12 | 工具发现 | `initialize` 成功后立即调用 `tools/list`，将工具清单写入健康结果与状态缓存 | ✅ |
| 13 | 工具订阅 | 订阅 `notifications/tools/list_changed` | 🔜 后续 PR |

### 2.2 数据模型

```rust
// ── MCP 服务器配置 ──
pub struct McpServerConfig {
    pub id: String,           // JSON 顶层 key
    pub name: String,         // = id
    pub enabled: bool,
    pub transport: McpTransportConfig,
}

pub enum McpTransportConfig {
    Stdio { command, args, env },
    Http { url, headers, oauth: Option<OAuthClientConfig> },
}

pub struct OAuthClientConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
}

// ── 状态机（对标 Zed ContextServerState） ──
pub enum McpServerState {
    Starting,                                    // 正在启动
    Running { tools: Vec<ToolInfo> },             // 运行中 + 已发现工具
    Stopped,                                      // 已停止
    Error { message: String },                    // 启动失败
    AuthRequired { auth_url: Option<String> },    // 需要 OAuth
}

// ── 工具信息 ──
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

// ── 健康检查结果 ──
pub struct McpServerHealthResult {
    pub status: String,        // "healthy" | "unhealthy" | "auth_required"
    pub message: Option<String>,
    pub auth_url: Option<String>,
    pub needs_client_secret: Option<bool>,
    pub tools: Vec<ToolInfo>,   // tools/list 结果（仅 healthy 时填充）
}
```

### 2.3 健康检查协议

**统一 MCP initialize 握手（Stdio + HTTP 共享）：**

```rust
fn build_initialize_request() -> Value    // 构建标准 MCP initialize JSON-RPC
fn parse_initialize_response(&str) -> Result<McpServerHealthResult>  // 解析响应
```

**Stdio 流程：**
```
spawn command → stdin.write(initialize) → 读取匹配 id=1 的 initialize 响应 → stdin.write(tools/list) → 读取匹配 id=2 的工具响应 → kill
```

**HTTP 流程：**
```
POST url body=initialize_request → 200: parse response / 401: OAuth discovery → result
```

### 2.4 健康门控与缓存

```rust
// to_acp_mcp_servers() — 缓存优先
pub fn to_acp_mcp_servers(&self) -> Result<Vec<Value>> {
    // 1. 检查 state_cache: Running → 直接通过
    // 2. 缓存未命中 → verify_server() → 更新缓存
    // 3. 仅返回 status=="healthy" 的服务器
}

// check_health() — 手动刷新并更新缓存
pub fn check_health(&self, id: &str) -> Result<McpServerHealthResult>;

// refresh_health() — 对标 Zed wait_for_context_server
pub fn refresh_health(&self, id: &str) -> Result<McpServerHealthResult>;

// invalidate_health() — 清除缓存
pub fn invalidate_health(&self, id: &str);
```

### 2.5 运行时链路

```
1. UI 配置 MCP → settings.json
2. node_executor 创建 McpManager → render_mcp_tools_catalog() → {{mcp_tools}}
3. node_executor 调用 to_acp_mcp_servers() → 健康门控 → mcp_servers
4. provider 传递 &req.mcp_servers → ACP session/new { mcpServers: [...] }
5. ACP Agent 直连 MCP 服务器（路径 B — 不经过 Gold-Band 中转）
```

### 2.6 Tauri Commands

| Command | 输入 | 输出 |
|---------|------|------|
| `list_mcp_servers` | — | `Vec<McpServerVm>` |
| `add_mcp_server` | `jsonContent: String` | `Vec<McpServerVm>` |
| `update_mcp_server` | `id, jsonContent` | `Vec<McpServerVm>` |
| `delete_mcp_server` | `id` | `Vec<McpServerVm>` |
| `toggle_mcp_server` | `id, enabled` | `Vec<McpServerVm>` |
| `check_mcp_server_health` | `id` | `McpServerHealthResult` |
| `refresh_mcp_health` | `id` | `McpServerHealthResult` |
| `invalidate_mcp_health` | `id` | — |

### 2.7 UI 特性

- 搜索：按名称/command/url 过滤
- 状态指示灯：🟢 healthy / 🟡 auth_required / 🔴 unhealthy / ⚪ unchecked
- 状态统计条：healthy/auth/error 数量
- enabled 开关：❌→✅ 自动触发健康检查，✅→❌ 清除状态
- 保存 Sheet：保持打开 → "正在连接…" → 成功关闭 / 失败显示具体错误（6 秒自动消失 + ✕ 手动关闭）
- 诊断按钮：每个服务器卡片的"MCP 服务诊断"按钮
- 进入 Tab 时自动刷新 + 检查所有 enabled 服务器

### 2.8 Zed 对标达成度

| 能力 | 状态 |
|------|------|
| 统一 MCP initialize 握手（Stdio + HTTP） | ✅ |
| 状态机 `McpServerState` | ✅ |
| 后端健康状态缓存（RefCell + HashMap） | ✅ |
| `list()` 返回实际健康状态 | ✅ |
| `to_acp_mcp_servers()` 缓存优先 + 健康门控 | ✅ |
| 手动刷新/失效 | ✅ |
| System prompt 渲染工具列表（缓存优先） | ✅ |
| 多行响应处理 + 10s 超时保护 | ✅ |
| 长期进程管理 | 🔜 |
| `tools/list` 自动发现 | ✅ |
| `tools/list_changed` 订阅 | 🔜 |

---

## 三、SKILL 管理

### 3.1 核心决策

| # | 决策 | 结论 | 对标 Zed |
|---|------|------|----------|
| 1 | 存储模型 | `.agents/skills/` 文件系统（全局 + 项目级） | ✅ |
| 2 | Scope 选择 | 创建时 Dropdown 显式选择 Global / Project | ✅ |
| 3 | 默认 Scope | 有 workspace 时默认 Project，无时默认 Global | ✅ |
| 4 | 编辑限制 | 编辑时 Scope 锁定 | ✅ |
| 5 | 重名检测 | 实时，冲突时禁用保存 + 红色错误提示 | ✅ |
| 6 | 改名处理 | 编辑改名先写新文件再删旧文件（oldName 参数） | ✅ |
| 7 | 渲染 | View 模式 Markdown 渲染 | ✅ |
| 8 | 传递方式 | System Prompt `{{skill_catalog}}` → ACP `_meta.systemPrompt.append` | ✅ |
| 9 | Body 嵌入 | SKILL.md 正文直接注入 system prompt（路径 A） | ✅ 替代 SkillTool |
| 10 | 跨源优先级 | `apply_skill_overrides()`: Project(2) > Global(1) > BuiltIn(0) | ✅ |
| 11 | Token 预算 | `select_catalog_skills()`: 50KB catalog budget | ✅ |
| 12 | 项目隔离 | 仅加载当前 workspace 的项目 SKILL | ✅ |
| 13 | 信任门控 | 本地自动信任 + 外部来源弹窗 + settings.json | 🔜 |

### 3.2 数据模型

```rust
// ── SKILL 元数据 ──
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub directory_path: String,
    pub disable_model_invocation: bool,
    pub load_warnings: Vec<String>,
}

pub enum SkillSource {
    BuiltIn,  // 内置（暂未实现）
    Global,   // ~/.agents/skills/
    Project,  // <workspace>/.agents/skills/
}

// ── 优先级（对标 Zed SkillSource::precedence） ──
fn precedence(source: SkillSource) -> u8 {
    match source {
        BuiltIn => 0,
        Global => 1,
        Project => 2,  // 最高优先级
    }
}
```

### 3.3 文件系统布局

```
~/.agents/skills/                    ← 全局 SKILL（所有项目可用）
  └── <name>/SKILL.md

<workspace>/.agents/skills/           ← 项目级 SKILL（仅当前 project）
  └── <name>/SKILL.md
```

### 3.4 SKILL.md 格式

```markdown
---
name: my-skill
description: A helpful skill for doing X
---

具体技能指引内容...
```

- 前置元数据（`---` 分隔）: `name`, `description`, `disable-model-invocation`
- 正文: 自由 Markdown
- 文件大小限制: 100KB
- 描述长度限制: 1024 字节

### 3.5 运行时集成

```
1. SkillManager::catalog_skills_for_agent_workspace(path)
   → 全局 SKILL + 当前 workspace 项目 SKILL
   → apply_skill_overrides() 优先级去重
   → select_catalog_skills() 50KB 预算截断

2. SkillManager::render_skill_catalog_for_workspace(lang, path)
   → 读取每个 SKILL 的 body
   → MiniJinja 渲染 skill_catalog_block.md 模板
   → 注入 system prompt {{skill_catalog}}

3. System Prompt 中 SKILL 内容格式:
   <available_skills> — 目录摘要（name + description + location）
   <skill_instructions> — 完整 body（Agent 可直接使用）
```

### 3.6 System Prompt 模板

**对标 Zed `system_prompt.hbs:221-248`：**

```xml
{{#if has_skills}}
## Agent Skills

You have access to the following Skills — modular capabilities...

<available_skills>
{{#each skills}}
  <skill>
    <name>{{name}}</name>
    <description>{{description}}</description>
    <location>{{directory_path}}</location>
  </skill>
{{/each}}
</available_skills>

<skill_instructions>
{{#each skills}}
### {{name}}
{{body}}
{{/each}}
</skill_instructions>
{{/if}}
```

### 3.7 Tauri Commands

| Command | 输入 | 输出 |
|---------|------|------|
| `list_skills` | — | `SkillListVm { global, project }` |
| `list_project_skills` | `workspacePath` | `Vec<SkillMetaVm>` |
| `read_skill` | `name, source, workspacePath?` | `SkillContentVm` |
| `write_skill` | `name, source, content, workspacePath?, oldName?` | `SkillListVm` |
| `delete_skill` | `name, source, workspacePath?` | `SkillListVm` |

### 3.8 UI 特性

- 全局 Tab：直接展示所有全局 SKILL + 搜索
- 项目 Tab：必须选择 workspace 才展示 + workspace 选择器下拉
- 创建/编辑：Sheet 表单，Scope 编辑时锁定
- 重名检测：实时计算，Save disabled + 红色错误
- View 模式：Markdown 渲染
- 卡片：View / Edit / Delete 操作按钮 + Tooltip

### 3.9 Zed 对标达成度

| 能力 | 状态 | 替代方案 |
|------|------|----------|
| 跨源优先级去重 | ✅ | `apply_skill_overrides()` |
| SKILL body 嵌入 | ✅ | 路径 A: system prompt 全量注入 |
| Zed 模板格式 | ✅ | `has_skills` + `<available_skills>` |
| disable_model_invocation | ✅ | |
| 50KB Token 预算 | ✅ | `select_catalog_skills()` |
| 项目隔离 | ✅ | `catalog_skills_for_agent_workspace()` |
| SkillTool (Agent 工具) | ❌ | 路径 A 内嵌替代 |
| 斜杠命令 | ❌ | ACP 架构不支持 |
| SKILL Mention | ❌ | ACP 架构不支持 |
| File Watch 自动刷新 | 🔜 | 手动刷新 |
| 信任门控 | 🔜 | C+1 方案 |

---

## 四、前端架构

### 4.1 ContextManagementPage

```
Page
├── PageHeader（标题）
├── Tab 条（角色管理 / MCP 管理 / SKILL 管理）
├── Profiles Tab → activeTab === 'profiles'
├── MCP Tab    → activeTab === 'mcp'
│   ├── 搜索栏 + 状态统计（healthy/auth/error 数量）
│   ├── MCP 卡片网格（ScrollArea）
│   │   └── 每卡片: 状态灯 + 名称 + transport + 操作按钮
│   └── 诊断按钮（Stethoscope → "MCP 服务诊断"）
├── SKILL Tab  → activeTab === 'skills'
│   ├── 全局/项目 Tabs + workspace 选择器
│   ├── 搜索栏
│   └── SKILL 卡片网格（ScrollArea）
├── McpSheet / SkillSheet
│   ├── 错误 banner（6 秒自动消失 + ✕ 手动关闭）
│   └── 保存（Close 时自动清 error）
└── 删除确认 Dialogs
```

### 4.2 组件复用

| 组件 | 用途 |
|------|------|
| `Card` / `AppCard` / `ScrollArea` | 卡片容器 + 滚动 |
| `Sheet` / `AlertDialog` | 编辑面板 / 确认对话框 |
| `Tabs` / `Select` / `Input` / `Textarea` | 导航和表单 |
| `Tooltip` / `Badge` | 提示和标记 |
| `Markdown` | SKILL View 渲染 |
| `Loader2` / `Stethoscope` / `Check` | 状态图标 |

### 4.3 错误处理

- MCP 保存失败 → 显示具体原因（`message` 字段）+ 6 秒自动消失 + ✕ 按钮
- Sheet 关闭时自动清除错误状态（`dismissMcpSheet()`）
- API 调用异常被外层 `try/catch` 捕获并展示

### 4.4 类型定义

```typescript
interface McpServerVm {
  id, name, enabled, transport, command?, args?, env?, url?, headers?
  healthStatus?: 'healthy' | 'unhealthy' | 'auth_required' | 'stopped' | 'checking' | 'unknown' | null
  healthMessage?: string | null
}

interface McpServerHealthResult {
  status: 'healthy' | 'unhealthy' | 'auth_required' | 'unknown'
  message?, authUrl?, needsClientSecret?
}

interface ToolInfo {
  name: string
  description?: string | null
  inputSchema?: Record<string, unknown> | null
}
```

---

## 五、存储布局

```
~/.gold-band/
├── settings.json          ← MCP 配置（context_servers 字段）
│                            + 信任列表（trusted_workspaces 字段）

~/.agents/skills/          ← 全局 SKILL
  └── <name>/SKILL.md

<workspace>/.agents/skills/ ← 项目级 SKILL（每 workspace 独立）
  └── <name>/SKILL.md
```

---

## 六、完整文件清单

### 6.1 Rust 后端

| 文件 | 变更类型 | 说明 |
|------|----------|------|
| `src/config/mod.rs` | 修改 | +`McpServerState` +`ToolInfo` +`McpServerHealthResult.tools` +`SkillMeta` +`SkillSource` +常量 |
| `src/mcp/mod.rs` | **新增** | 514→~650 行: `McpManager` + 协议握手 + 状态机 + 缓存 + ACP 序列化 + catalog 渲染 + 超时保护 |
| `src/skill/mod.rs` | **新增** | ~290→~350 行: `SkillManager` + CRUD + 优先级去重 + body 嵌入 + workspace 隔离 + 预算保护 |
| `src/storage/mod.rs` | 修改 | `GoldBandPaths` 新增 global/project SKILL 目录方法 |
| `src/lib.rs` | 修改 | 注册 `pub mod mcp` + `pub mod skill` |
| `src/app/mod.rs` | 修改 | 委托方法 + 删除死代码（~100 行重复类型/函数） |
| `src/app/node_executor.rs` | 修改 | `build_worker_invocation`: MCP/SKILL catalog + mcp_servers + workspace 隔离 |
| `src/acp/client.rs` | 修改 | `session_new_params` / `session_load_params` 接入 `mcpServers` |
| `src/provider/mod.rs` | 修改 | `WorkerInvocation` + `mcp_servers` 字段 + AcpProvider 传递修复 + 模板变量修正 |
| `src/prompts.rs` | 修改 | `SKILL_CATALOG_BLOCK_*` 常量 |
| `src/prompts/{en,zh-CN}/runtime/system.md` | 修改 | `{{skill_catalog}}` `{{mcp_tools}}` 占位符 |
| `src/prompts/{en,zh-CN}/runtime/skill_catalog_block.md` | **新增** | Zed 模板对齐: `has_skills` + `<available_skills>` + `<skill_instructions>` |
| `src-tauri/src/commands.rs` | 修改 | 11 个 MCP/SKILL commands + 2 个 ACP mcp_servers 传递点 |
| `src-tauri/src/view_models.rs` | 修改 | ViewModels + 转换函数 |
| `src-tauri/src/main.rs` | 修改 | 注册新 commands |
| `Cargo.toml` | 修改 | `reqwest` + `url` 依赖 |
| `tests/provider_prompt_bundle.rs` | 修改 | 补充缺失字段 |

### 6.2 前端

| 文件 | 变更类型 | 说明 |
|------|----------|------|
| `web/src/types.ts` | 修改 | MCP/SKILL 类型定义 + `ToolInfo` + `healthStatus` 类型修复 |
| `web/src/api.ts` + `desktop.ts` + `client.ts` + `browser.ts` | 修改 | API 层 |
| `web/src/pages/ContextManagementPage.tsx` | 修改 | 三个 Tab + MCP JSON 编辑器 + SKILL 表单 + 健康状态 + 错误处理优化 |
| `web/src/i18n.ts` | 修改 | 中英文文案 + `errors.app.unexpected` 增加 `{{message}}` + `diagnoseServer` key |

---

## 七、数据流总览

### 7.1 MCP 运行时数据流

```
settings.json
  → McpManager::enabled_servers()               [过滤 enabled]
    → state_cache 检查                          [缓存优先]
      → verify_server()                         [缓存未命中: MCP initialize 握手]
        → McpServerState::Running{tools}        [更新缓存]
          → to_acp_mcp_servers()                [仅 healthy]
            → WorkerInvocation.mcp_servers      [结构化配置]
              → AcpProvider                     [&req.mcp_servers]
                → client::run_prompt()          [ACP session/new]
                  → Agent 直连 MCP             [路径 B: 不中转]
```

### 7.2 SKILL 运行时数据流

```
磁盘 (.agents/skills/<name>/SKILL.md)
  → scan_skills_dir()                           [扫描目录 + 解析前置元数据]
    → SkillManager::list()                      [global + project 分离]
      → catalog_skills_for_agent_workspace()    [按 workspace 过滤]
        → apply_skill_overrides()               [Project > Global 去重]
          → select_catalog_skills()             [50KB 预算截断]
            → render_skill_catalog_for_workspace()
              → read_body_for_meta()            [读取 SKILL.md 正文]
                → MiniJinja 渲染模板            [has_skills + body 嵌入]
                  → WorkerInvocation.skill_catalog
                    → system.md {{skill_catalog}}
                      → ACP _meta.systemPrompt.append
                        → Agent 收到完整 SKILL 指令
```

---

## 八、与 Zed 的完整差异矩阵

| 功能领域 | 能力 | Zed | Gold-Band | 差距 |
|----------|------|-----|-----------|------|
| **MCP — 配置** | JSON 编辑器 | ✅ | ✅ | — |
| | Stdio + HTTP 传输 | ✅ | ✅ | — |
| | OAuth 支持 | ✅ | ✅ (simplified) | 小幅 |
| **MCP — 健康** | initialize 握手 | ✅ | ✅ | — |
| | 统一协议 (HTTP 也发 initialize) | ✅ | ✅ | — |
| | 状态机 | ✅ (7 states) | ✅ (5 states) | 小幅 |
| | 状态缓存 | ✅ (内存) | ✅ (RefCell) | — |
| | 工具发现 (tools/list) | ✅ | 🔜 | 待实施 |
| | 工具订阅 (list_changed) | ✅ | 🔜 | 待实施 |
| | 长期进程 | ✅ | 🔜 | 待实施 |
| **MCP — 传递** | ACP mcpServers | ✅ | ✅ | — |
| | System Prompt 工具列表 | ✅ (cached) | ✅ (cached) | — |
| | 健康门控 | ✅ | ✅ | — |
| **SKILL — 管理** | 文件系统存储 | ✅ | ✅ | — |
| | 全局 + 项目级 | ✅ | ✅ | — |
| | 前置元数据解析 | ✅ | ✅ | — |
| | SKILL.md 编辑 UI | ✅ | ✅ | — |
| **SKILL — 传递** | System Prompt 目录 | ✅ | ✅ (Zed 模板) | — |
| | Body 嵌入 | ✅ (lazy via SkillTool) | ✅ (eager 全量) | 不同路径 |
| | 优先级去重 | ✅ | ✅ | — |
| | Token 预算 | ✅ (50KB) | ✅ (50KB) | — |
| | 项目隔离 | ✅ (ProjectState) | ✅ (workspace filter) | — |
| **SKILL — 调用** | SkillTool (Agent 工具) | ✅ | ❌ | 路径 A 替代 |
| | 斜杠命令 | ✅ | ❌ | ACP 约束 |
| | Mention 附件 | ✅ | ❌ | ACP 约束 |
| | Body 懒加载 | ✅ | ❌ | eager 替代 |
| **SKILL — 安全** | 项目信任门控 | ✅ | 🔜 | 待实施 |
| | XML envelope 转义 | ✅ | N/A (无 envelope) | — |

---

## 九、后续规划

### Phase 1 (本次已完成) ✅
- MCP 配置管理 (CRUD + JSON 编辑器 + 健康检查)
- MCP ACP 传递链路修复 (mcpServers 不再为空)
- SKILL 配置管理 (CRUD + 文件系统)
- SKILL System Prompt 注入 (Zed 模板格式 + body 嵌入)
- 优先级去重 + Token 预算 + 项目隔离
- 前端 UI (三个 Tab + 错误处理优化)
- 协议统一 (HTTP 发合法 MCP initialize + 多行响应)

### Phase 2 (后续 PR)
- [ ] 长期进程管理 (Stdio 进程保持存活)
- [ ] `tools/list` 自动发现
- [ ] `tools/list_changed` 订阅
- [ ] 信任门控 (C+1 方案: 本地自动信任 + 外部弹窗 + settings.json)

### Phase 3 (远期)
- [ ] BuiltIn SKILL 支持
- [ ] File watch 自动刷新
- [ ] AI-DYNAMIC 节点 MCP/SKILL 覆盖

---

> 基于 5 轮深度访谈 | 6 个规格文档 | 25 个文件代码变更
> 生成日期：2026-06-11 | 最终歧义度: < 5%
