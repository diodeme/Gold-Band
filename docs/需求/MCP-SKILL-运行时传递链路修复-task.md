# Task: MCP & SKILL 运行时传递链路修复

> 基于 Deep Interview Spec: `maling-deep-interview-mcp-skill-fix-wire.md`
> 歧义度: 8% | 轮次: 4 | 类型: 存量项目修复

---

## 一、需求概述

修复 Gold-Band 中 MCP 和 SKILL 的运行时传递链路。当前 MCP 配置管理（UI 编辑、JSON 解析、健康检查）已完整实现，但配置好的 MCP 服务器从未被实际传递给 ACP Agent。

### 核心问题

| # | 问题 | 位置 | 影响 |
|---|------|------|------|
| 1 | MCP 服务器配置未传递给 ACP Agent | `provider/mod.rs:354` 硬编码 `&[]` | Agent 收不到任何 MCP 工具 |
| 2 | SKILL prompt 引用不存在的工具 | `skill_catalog_block.md` 指引使用 `skill` 工具 | Agent 可能尝试调用失败 |
| 3 | 无健康门控 | `to_acp_mcp_servers()` 不过滤不健康服务器 | 不稳定服务器可能影响 Agent 会话 |

---

## 二、目标

1. 启用且健康检查通过的 MCP 服务器正确传递到 ACP Agent
2. SKILL system prompt 诚实反映当前能力（移除 "使用 skill 工具" 误导指引）

---

## 三、约束与边界

- ✅ **在范围内**：
  - 修复 `provider/mod.rs:354` → 传递实际 MCP 配置
  - `to_acp_mcp_servers()` 增加健康过滤
  - 修正中英文 `skill_catalog_block.md` 提示词
  - `WorkerInvocation` 新增 `mcp_servers` 字段
  - `node_executor.rs` 传递 MCP 配置

- ❌ **不在范围内**：
  - MCP 工具发现（`tools/list`）机制
  - `SkillTool` 实现
  - AI-DYNAMIC 节点（`orchestrator.rs:3743-3744`）
  - Agent 端 MCP 连接状态监控

---

## 四、实施步骤

### Step 1: `WorkerInvocation` 新增 `mcp_servers` 字段
**文件：** `src/provider/mod.rs`

在 `WorkerInvocation` 结构体中新增：
```rust
#[serde(default)]
pub mcp_servers: Vec<serde_json::Value>,
```

### Step 2: `McpManager::to_acp_mcp_servers()` 增加健康过滤
**文件：** `src/mcp/mod.rs`

修改 `to_acp_mcp_servers()` 方法，仅返回 enabled + healthy 的服务器：

```rust
pub fn to_acp_mcp_servers(&self) -> Result<Vec<Value>> {
    Ok(self
        .enabled_servers()?
        .into_iter()
        .filter(|s| {
            self.verify_server(s)
                .map(|r| r.status == "healthy")
                .unwrap_or(false)
        })
        .map(|s| match &s.transport { ... })  // 现有序列化逻辑不变
        .collect())
}
```

### Step 3: `node_executor` 传递 MCP 配置
**文件：** `src/app/node_executor.rs`

在 `build_worker_invocation()` 中：
```rust
let mcp_mgr = crate::mcp::McpManager::new(app.paths.user_settings_file());
let mcp_tools_catalog = mcp_mgr.render_mcp_tools_catalog();
let mcp_servers = mcp_mgr.to_acp_mcp_servers().unwrap_or_default();
// ... 传入 WorkerInvocation { mcp_servers, ... }
```

### Step 4: `AcpProvider` 使用 `mcp_servers`
**文件：** `src/provider/mod.rs`

```rust
// Before:
&[], // TODO: pass MCP servers from App context

// After:
&req.mcp_servers,
```

### Step 5: 修正 SKILL catalog 提示词
**文件：** `src/prompts/en/runtime/skill_catalog_block.md`
**文件：** `src/prompts/zh-CN/runtime/skill_catalog_block.md`

移除 "To use a Skill:" / "如何使用 Skill:" 操作指引段落。

英文版修改后：
```xml
## Agent Skills

You have access to the following Skills — modular capabilities that provide specialized instructions for specific tasks.

<available_skills>
{{#each skills}}
  <skill>
    <name>{{name}}</name>
    <description>{{description}}</description>
    <location>{{directory_path}}</location>
  </skill>
{{/each}}
</available_skills>
```

### Step 6: 更新测试
**文件：** `src/provider/mod.rs`

更新 `render_prompt_bundle_does_not_add_builtin_output_contracts` 测试，添加 `mcp_servers: Vec::new()` 字段。

---

## 五、涉及文件清单

| 文件 | 变更类型 | 说明 |
|------|----------|------|
| `src/provider/mod.rs` | 修改 | WorkerInvocation + mcp_servers 字段 + AcpProvider 调用修复 + 测试更新 |
| `src/mcp/mod.rs` | 修改 | to_acp_mcp_servers() 增加健康过滤 |
| `src/app/node_executor.rs` | 修改 | build_worker_invocation() 传递 mcp_servers |
| `src/prompts/en/runtime/skill_catalog_block.md` | 修改 | 移除 "use skill tool" 指引 |
| `src/prompts/zh-CN/runtime/skill_catalog_block.md` | 修改 | 同步中文版修改 |

---

## 六、验收标准

- [ ] `AcpProvider::run_worker_with_live_update()` 传递实际 MCP 配置（不再 `&[]`）
- [ ] ACP `session/new` 的 `mcpServers` 仅包含 enabled + healthy 服务器
- [ ] ACP `session/load` 的 `mcpServers` 同步过滤
- [ ] 健康检查失败的 enabled 服务器被静默跳过
- [ ] 中英文 `skill_catalog_block.md` 不包含 "使用 `skill` 工具" 指引
- [ ] `build_worker_invocation()` 正确传递 `mcp_servers`
- [ ] 已有测试继续通过

---

## 七、参考资料

- 规格文档：`.maling/specs/maling-deep-interview-mcp-skill-fix-wire.md`
- 需求设计：`docs/gold-band/产品设计文档/MCP-SKILL管理-需求设计方案.md`
- Zed 参考：`crates/agent_servers/src/acp.rs` — `into_new_session_request().mcp_servers()`
- Zed 参考：`crates/project/src/context_server_store.rs` — ContextServerStore
- Zed 参考：`crates/agent/src/tools/context_server_registry.rs` — ContextServerRegistry

---

> Generated: 2026-06-11 | From: Deep Interview `mcp-skill-fix-wire` | Status: ready for implementation
