# 任务文档: Gold-Band SKILL 多 Agent 管理改进

> 来源规格: `.maling/specs/maling-deep-interview-skill-multi-agent.md`
> 歧义度: 8% | 访谈轮数: 8 | 日期: 2026-06-26

---

## 任务概述

将 Gold-Band 的 SKILL 管理从"仅 Claude 单 agent + system prompt 注入"升级为"多 Agent 目录识别 + 软链同步 + UI 展示 + 移除冗余注入"。
## 最新实施说明
- 已将 SKILL 管理收敛为实例级目录模型：列表按 `directoryPath` 识别，同名原生 skill 允许并列展示。
- 同步目标以全局已配置 agent 为准；保存时按目标集合对账，支持“只保存不同步”与“取消已存在同步”。
- 若目标目录已有同名原生 skill，则直接阻止并提示冲突，不再覆盖；同目录同名创建/重命名也会直接阻止。
- 列表查询仅展示各目录中的原生 skill，软链副本不单独展示；卡片左下角展示来源 agent 图标。


---

## 任务分解

### 任务 1: ManagedAgentType 增加 skills 目录映射

**目标**: 为每个 agent 类型定义其 skills 目录，参考 cc-switch 混合方案

**变更文件**:
- `src/config/mod.rs`

**具体步骤**:
1. `ManagedAgentType` 新增 `skills_dir_name()` 方法，返回硬编码默认值：
   - `ClaudeAcp` → `".claude"`
   - `CodexAcp` → `".codex"`
   - `Cursor` → `".cursor"`
   - `Gemini` → `".gemini"`
   - `OpenCode` → `".opencode"`
2. `ManagedAgentConfig` 新增 `skills_dir_override: Option<String>` 字段
3. 新增 `skills_dir()` 方法：`skills_dir_override` || 硬编码默认值
4. 新增 `skills_dir_path()` 方法：`{home}/{skills_dir()}/skills/` 或 `{workspace}/{skills_dir()}/skills/`

**验收标准**:
- [ ] `ManagedAgentType::skills_dir_name()` 返回正确的目录名
- [ ] `ManagedAgentConfig::skills_dir()` 优先使用 override
- [ ] `skills_dir_path(home, workspace)` 返回完整路径

---

### 任务 2: SkillMeta 数据结构更新

**目标**: 新增 `agent_source` 字段，移除 `disable_model_invocation`

**变更文件**:
- `src/config/mod.rs`
- `src-tauri/src/view_models.rs`
- `web/src/types.ts`
- `web/src/api/desktop.ts`
- `web/src/api.ts`

**具体步骤**:
1. `SkillMeta` 新增 `agent_source: String` 字段（必填）
   - Gold-Band 自行创建/管理的 SKILL 标注为 `".agents"`
   - 其他 agent 目录扫描到的标注为 `".claude"`、`".codex"` 等
2. 删除 `SkillMeta.disable_model_invocation` 字段
3. 同步更新 `SkillMetaVm`（view model）
4. 同步更新前端 TypeScript 类型 `SkillMetaVm`
5. 前端删除 `disableModelInvocation` 相关 UI（Badge、toggle 开关）

**验收标准**:
- [ ] `SkillMeta.agent_source` 编译通过
- [ ] `SkillMetaVm` 前后端类型一致
- [ ] `disable_model_invocation` 全局无引用

---

### 任务 3: SkillManager 重构 — 多 Agent 目录扫描

**目标**: `list()` 扫描 `.agents/skills/` + 所有已配置 agent 的 skills 目录

**变更文件**:
- `src/skill/mod.rs`
- `src/config/mod.rs`

**具体步骤**:
1. 新增辅助函数 `get_configured_agent_dirs(settings, home, workspace)` — 从 `SettingsConfig.agents` 推导 agent skills 目录列表
2. 重构 `scan_skills_dir()` → 接受 `agent_source: &str` 参数
3. 重构 `SkillManager::list()`:
   - 扫描 `.agents/skills/` → 标记 `agent_source = ".agents"`
   - 遍历已配置 agent → 扫描各 agent skills 目录 → 标记对应 `agent_source`
   - 严格过滤：agent 在 settings 中已配置 **AND** 目录实际存在
4. **双来源去重**: `.agents` 管理的 SKILL 与 agent 目录扫描结果取并集，`.agents` 优先（按目录路径前缀判断）
5. 同样更新 `list_by_workspace()` 支持项目级多 agent 扫描
6. 删除以下函数：
   - `catalog_skills()` / `catalog_skills_for_workspace()`
   - `catalog_skills_for_agent()` / `catalog_skills_for_agent_workspace()`
   - `render_skill_catalog()` / `render_skill_catalog_for_workspace()`
   - `select_catalog_skills()` / `apply_skill_overrides()` / `MAX_CATALOG_BYTES`
7. 删除 `parse_skill_md()` 中 `disable_model_invocation` 解析逻辑

**验收标准**:
- [ ] `list()` 返回包含 `.agents` + 所有已配置 agent 的 SKILL 列表
- [ ] 严格过滤：未配置 agent 的目录不被扫描
- [ ] 同名 SKILL：`.agents` 版本保留，agent 版本丢弃
- [ ] agent 目录不存在时不报错（静默跳过）
- [ ] catalog 相关函数全部删除，`cargo check` 无引用

---

### 任务 4: 同名冲突检测（创建时警告）

**目标**: 创建新 SKILL 时检测与 agent 目录中同名 SKILL 的冲突

**变更文件**:
- `src/skill/mod.rs`
- `src-tauri/src/commands.rs`（`write_skill` 命令）
- `web/src/pages/ContextManagementPage.tsx`

**具体步骤**:
1. 在 `SkillManager` 中新增 `check_name_conflict(name, settings)` 方法 — 返回所有与该名称冲突的 agent 目录列表
2. 前端创建 SKILL 时，先调用冲突检测
3. 若有冲突，直接阻止保存并提示冲突目录；不再提供“继续覆盖”分支
4. 用户确认后继续创建

**验收标准**:
- [ ] 创建与同步目标目录中同名原生 SKILL 时直接阻止
- [ ] 同目录同名创建或重命名直接阻止
- [ ] 无冲突时才允许保存

---

### 任务 5: symlink 同步 — 多目标支持

**目标**: `symlink::sync_all()` 支持向多个 agent 目录创建/删除软链

**变更文件**:
- `src/skill/symlink.rs`
- `src/app/mod.rs`（`sync_skill_instance` / `cleanup_skill_instance_links`）
- `src-tauri/src/commands.rs`（`write_skill`、`delete_skill` 命令）

**具体步骤**:
1. 重构 `sync_all()` 签名 — 接受 `target_agents: Vec<ManagedAgentType>` 参数
2. 为每个目标 agent 调用 `sync_to_target()`（agent skills 目录作为 target_dir）
3. 同步逻辑按目标集合对账：创建缺失链接、删除未勾选且指向当前实例的既有链接
4. 删除 SKILL 时，遍历所有已配置 agent 目录清理对应软链
5. `App::sync_skill_instance()` 接受 `source_directory_path` 和 target_agents 参数
6. Tauri `write_skill` / `delete_skill` 命令按实例目录同步，并传入用户选择的同步目标

**验收标准**:
- [ ] 创建 SKILL 时可选择同步到 Claude、Codex、Cursor、Gemini、OpenCode 中的任意组合
- [ ] 同步后在目标 agent 目录下出现正确的软链/junction
- [ ] 删除 SKILL 后所有目标 agent 目录中的对应软链被清理
- [ ] Unix: 使用 `symlink()`，Windows: `symlink_dir()` → `mklink /J` 降级

---

### 任务 6: 同步目标 UI — 前端多选 + 软链反推

**目标**: 创建/编辑 SKILL Sheet 新增同步目标多选，编辑时扫描软链反推状态

**变更文件**:
- `web/src/pages/ContextManagementPage.tsx`
- `web/src/i18n.ts`
- `src-tauri/src/commands.rs`（新增 `get_skill_sync_status` 命令）

**具体步骤**:
1. 创建 SKILL Sheet 新增“同步目标”区域 —— 仅展示全局已配置 agent 的多选列表，允许全部取消以实现“只保存不同步”
2. 编辑 SKILL 时，新增 Tauri 命令 `get_skill_sync_status(name, source)`：
   - 扫描各已配置 agent 的 skills 目录
   - 检查是否存在指向该 SKILL 的软链
   - 返回 `{ claude: true, codex: false, ... }` 状态
3. Sheet 根据反推结果初始化 checkbox 勾选状态
4. 保存时将用户选择的同步目标传入 `write_skill` 命令

**验收标准**:
- [ ] 创建 SKILL 时默认仅勾选 Claude
- [ ] 编辑时自动反推当前同步状态并勾选
- [ ] 用户修改勾选后保存生效
- [ ] 国际化文案中英文正确

---

### 任务 7: 移除 System Prompt 注入链路

**目标**: 删除整个 SKILL catalog 注入链路和 `disable_model_invocation`

**变更文件**:
- `src/skill/mod.rs`（删除 catalog 渲染函数）
- `src/app/node_executor.rs`（移除 `skill_catalog` 赋值）
- `src/provider/mod.rs`（移除 `skill_catalog` 字段）
- `src/prompts/en/runtime/skill_catalog_block.md`（删除）
- `src/prompts/zh-CN/runtime/skill_catalog_block.md`（删除）
- `src/prompts/en/runtime/system.md`（移除 `{{skill_catalog}}` 占位符）
- `src/prompts/zh-CN/runtime/system.md`（移除 `{{skill_catalog}}` 占位符）
- `src/prompts.rs`（移除 SKILL_CATALOG_BLOCK_* 常量）
- `src/config/mod.rs`（移除 `disable_model_invocation` 字段，已在任务 2 中完成）

**具体步骤**:
1. 删除 `render_skill_catalog()` / `render_skill_catalog_for_workspace()`
2. 删除 `src/prompts/{en,zh-CN}/runtime/skill_catalog_block.md`
3. 更新 `system.md` 移除 `{{skill_catalog}}` 占位符和相关条件渲染
4. `node_executor.rs` 中移除 `render_skill_catalog_for_workspace()` 调用，`WorkerInvocation.skill_catalog` 设为空字符串
5. 检查 `PromptBundle.skill_catalog` 是否可以移除或设空
6. 清理 `src/prompts.rs` 中的常量引用

**验收标准**:
- [ ] `skill_catalog_block.md` 两个语言版本均已删除
- [ ] `system.md` 中不再包含 `{{skill_catalog}}` 占位符
- [ ] `cargo check` 无未引用代码警告
- [ ] 编译后的 system prompt 中不包含 SKILL catalog 内容

---

### 任务 8: 前端 SKILL 卡片展示更新

**目标**: 卡片展示 agent 来源徽章 + 同名冲突时 `.agents` 优先

**变更文件**:
- `web/src/pages/ContextManagementPage.tsx`
- `web/src/i18n.ts`

**具体步骤**:
1. SKILL 卡片新增 agent 来源徽章（Badge），显示 `agent_source` 值（如 "`.agents`"、"`.claude`"、"`.codex`"）
2. 双来源去重已由后端处理（任务 3），前端直接渲染
3. 删除 `disableModelInvocation` 相关 Badge
4. 删除 SKILL Sheet 中的 `disableModelInvocation` toggle

**验收标准**:
- [ ] 每个 SKILL 卡片右下角显示来源 agent 徽章
- [ ] `.agents` 来源的 SKILL 优先展示
- [ ] 不再显示 "manual only" Badge

---

### 任务 9: 国际化 & 提示词清理

**目标**: 更新中英文 UI 文案和提示词

**变更文件**:
- `web/src/i18n.ts`
- `src/prompts/en/runtime/system.md`
- `src/prompts/zh-CN/runtime/system.md`

**具体步骤**:
1. 新增国际化 key:
   - `contextManagement.skills.syncTargets` — "同步目标" / "Sync Targets"
   - `contextManagement.skills.nameConflict` — 冲突警告文案
   - `contextManagement.skills.agentSource.*` — agent 名称
2. 删除国际化 key:
   - `contextManagement.skills.disableModelInvocation`
3. 删除中英文 `skill_catalog_block.md` 文件
4. 更新 `system.md` 移除 `{{skill_catalog}}` 段落

**验收标准**:
- [ ] 中英文 UI 文案完整无缺失
- [ ] system.md 中英文版均已清理

---

### 任务 10: 单元测试

**目标**: 为新增/修改的核心逻辑添加单元测试

**变更文件**:
- `src/skill/mod.rs`（添加 `#[cfg(test)] mod tests`）
- `src/skill/symlink.rs`（已有部分测试，更新）

**具体步骤**:
1. 测试 `ManagedAgentType::skills_dir_name()` 每个变体返回值
2. 测试 `ManagedAgentConfig::skills_dir()` 覆盖逻辑
3. 测试 `SkillManager::list()` 多目录扫描 + 去重
4. 测试同名冲突检测（模拟 agent 目录存在同名 SKILL）
5. 测试 `symlink::sync_all()` 多目标同步
6. 更新 symlink 已有测试（适配新签名）

**验收标准**:
- [ ] `cargo test` 全部通过
- [ ] 新增测试覆盖核心逻辑路径

---

## 执行依赖

```
任务1 (ManagedAgentType 映射)
├─→ 任务2 (SkillMeta 字段更新)
├─→ 任务3 (SkillManager 重构)
│   └─→ 任务4 (同名冲突检测)
│   └─→ 任务5 (symlink 多目标)
│       └─→ 任务6 (前端同步目标 UI)
├─→ 任务7 (移除 System Prompt 注入)
├─→ 任务8 (前端卡片展示更新)
├─→ 任务9 (国际化 & 提示词)
└─→ 任务10 (单元测试) [最后执行]
```

**推荐顺序**: 1 → 2 → 3 → 7 → 5 → 4 → 6 → 8 → 9 → 10

---

## 影响范围

| 层级 | 文件 | 变更类型 |
|------|------|----------|
| Rust core | `src/config/mod.rs` | 新增字段+方法，删除字段 |
| Rust core | `src/skill/mod.rs` | 重构 scan/list，删除 catalog 函数链 |
| Rust core | `src/skill/symlink.rs` | 重构支持多目标 |
| Rust app | `src/app/mod.rs` | 更新 `sync_skill_instance` / `cleanup_skill_instance_links` 接口 |
| Rust app | `src/app/node_executor.rs` | 删除 catalog 渲染 |
| Rust provider | `src/provider/mod.rs` | 清理 PromptBundle |
| Rust prompts | `src/prompts.rs` | 删除 catalog 常量 |
| Tauri | `src-tauri/src/commands.rs` | 新增 sync status 命令，更新 write/delete |
| Tauri | `src-tauri/src/view_models.rs` | 更新 SkillMetaVm |
| Prompts | `src/prompts/{en,zh-CN}/runtime/` | 删除 catalog_block，更新 system |
| Frontend | `web/src/pages/ContextManagementPage.tsx` | 新增 badge、同步多选、冲突警告、删除旧 UI |
| Frontend | `web/src/types.ts` | 更新类型定义 |
| Frontend | `web/src/i18n.ts` | 新增/删除文案 |
| Frontend | `web/src/api.ts` + `desktop.ts` | 更新 API 绑定 |
