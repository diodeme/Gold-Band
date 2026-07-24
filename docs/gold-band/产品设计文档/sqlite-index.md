# SQLite 辅助检索索引

## 定位

SQLite 在本项目中**仅用于辅助检索**，不承担：

- 会话详情渲染主存储
- 活跃会话 live state
- raw frame 排障
- timeline 恢复的唯一来源

文件仍然是权威事实源。删除 DB 文件不影响会话打开、详情渲染、恢复、排障任何功能。

## DB 位置

`{user_gold_band_root}/gold-band.db`

即 `~/.gold-band/gold-band.db`（全局，跨所有项目/workspace）。

## 一致性模型

```
文件写入成功 → 刷新 SQLite 派生索引 → 失败侧：重试3次，每次重新读文件 → 仍失败则 trace log 丢弃
```

- 文件永远先写，SQLite 后写。
- task 创建与元数据更新由 `App` 核心生命周期统一触发索引刷新；调用方不再分别维护索引写入，避免会话 UI、任务工作台等入口产生数据分叉。
- task 创建运行在已有 blocking 工作单元中，索引刷新完成后创建接口才返回；索引仍是 best-effort，最终失败只记录日志，不回滚已经成功落盘的权威文件。
- ACP session/prompt 索引继续通过 `spawn_blocking` 或 `std::thread::spawn` 后台刷新。
- `Mutex<Connection>` 仅持有事务期间，不在文件 I/O 期间持有。
- 重试间隔：200ms → 500ms → 1500ms，每次重新读取最新文件内容。

## 表设计

### `tasks`

存储任务元信息，用于跨项目检索 task。

| 列 | 类型 | 来源 | 说明 |
|---|---|---|---|
| `task_id` | TEXT | task 目录名 | workspace 内的本地任务 ID，不作为全局唯一键 |
| `task_path` | TEXT PK | 文件系统 | task 全局身份；不同 workspace 可同时存在同名 `task-001` |
| `title` | TEXT | `task.json` → `title` | |
| `description` | TEXT | `task.json` → `description` | |
| `requirement_text` | TEXT | `authoring/requirement.md` | 需求完整文本 |
| `created_at` | TEXT | — | 预留 |
| `updated_at` | TEXT | — | 预留 |

### `sessions`

每个 ACP attempt 一行会话摘要。

| 列 | 类型 | 来源 | 说明 |
|---|---|---|---|
| `attempt_path` | TEXT PK | 文件系统路径 | 全局唯一 |
| `session_id` | TEXT | `snapshot.adapter_id` | 适配器 ID |
| `task_id` | TEXT | 调用方传入 | 所属 task |
| `run_id` | TEXT | 调用方传入 | |
| `round_id` | TEXT | 调用方传入 | |
| `node_id` | TEXT | 调用方传入 | |
| `attempt_id` | TEXT | 调用方传入 | |
| `outer_node_id` | TEXT? | 调用方传入 | AI-Dynamic 父节点 |
| `outer_attempt_id` | TEXT? | 调用方传入 | AI-Dynamic 父 attempt |
| `title` | TEXT | `snapshot.title` | 会话标题 |
| `status` | TEXT | `snapshot.status` | running/completed/failed/cancelled |
| `created_at` | TEXT | `snapshot.created_at` | |
| `updated_at` | TEXT | `snapshot.updated_at` | |

### `session_prompts`

用户每次发送的 prompt（从 timeline 中提取 `userTextDelta`）。

| 列 | 类型 | 来源 | 说明 |
|---|---|---|---|
| `id` | TEXT | `timeline item.id` | 如 `gold-band-user-prompt-7` |
| `attempt_path` | TEXT | 文件系统路径 | 关联 sessions |
| `session_id` | TEXT | 同 sessions | |
| `prompt_id` | TEXT? | `item.raw.promptId` | 业务 prompt ID |
| `timestamp` | TEXT | `item.timestamp` | |
| `text` | TEXT | `item.content` | 用户原始输入 |
| `normalized_text` | TEXT | `text` 小写+空白折叠 | 搜索匹配用 |
| **PK** | | `(attempt_path, id)` | 跨 session 避免 ID 碰撞 |

## FTS5 全文索引

### `tasks_fts`

```sql
USING fts5(title, description, requirement_text, content=tasks, content_rowid=rowid)
```

通过 INSERT/UPDATE/DELETE 触发器自动同步 `tasks` 表。

### `session_prompts_fts`

```sql
USING fts5(text, content=session_prompts, content_rowid=rowid)
```

通过 INSERT/UPDATE/DELETE 触发器自动同步 `session_prompts` 表。

## 写入时机

| 触发点 | 索引内容 | 线程 |
|---|---|---|
| `App::create_task_from_requirement` | task 元信息 + requirement；覆盖任务工作台、会话 UI 等所有创建入口 | 当前任务创建的 blocking 工作单元 |
| `App::update_task_metadata` | 最新 title + description + requirement | 元数据更新的 blocking 工作单元 |
| `send_acp_prompt` 完成后 | session + prompts | `spawn_blocking` |
| `respond_acp_permission` 完成后 | session（重新读取最新文件） | `spawn_blocking` |
| `cancel_acp_session` 完成后 | session（重新读取最新文件） | `spawn_blocking` |
| orchestrator 节点执行完成后 | session + prompts | `std::thread::spawn` |

task 索引刷新属于 task 文件生命周期；ACP session/prompt 写入仍为 fire-and-forget。

若检测到 `sessions` / `session_prompts` 会话搜索索引 schema 版本落后，则启动时直接重建这组派生表与 FTS/trigger，再从文件系统回填；`tasks` 索引与源文件数据不受影响。

当前版本不主动修复历史 `tasks` 索引缺口。升级后新建或更新元数据的 task 会进入索引；既有未索引 task 只有在后续明确执行全量重建方案时才补齐。

schema v2 会把已有 `tasks` 表从 `task_id` 主键迁移为 `task_path` 主键，并原样保留数据库中已经存在的行；迁移不扫描文件系统、不补齐历史缺口。删除 task 时也按 `task_path` 清理对应 task/session/prompt 索引，不能按 workspace 内可能重复的 `task_id` 批量删除。

schema v3 将 task FTS tokenizer 从默认 `unicode61` 升级为 SQLite FTS5 内置 `trigram`。升级只用 `tasks` 表已有行重建派生 `tasks_fts`，不扫描 task 文件目录、不补齐历史未入库 task。该 tokenizer 用于支持中文、英文和中英混排内容的任意位置子串检索。

## 搜索接口

### `search_tasks`

搜索 task 的标题、描述、需求内容（FTS5 over `tasks_fts`）。返回 `TaskSearchResult`：

```jsonc
{
  "taskId": "...",
  "taskPath": "...",
  "title": "...",
  "description": "...",
  "requirementPreview": "前500字符...",
  "matchPreview": "包含当前搜索关键词的上下文摘要..."
}
```

桌面“搜索会话”接口在 `TaskSearchResult` 基础上根据 `task_path` 解析 workspace，并从文件事实源补齐最新 Run 摘要。没有最新 Run、所属 workspace 已不可用或无法形成有效会话路由的 task 不返回给会话搜索 UI，确保每条可见结果都能直接打开。

会话搜索的 workspace 范围是“当前侧边栏中的 workspace”：始终包含默认 workspace，并包含 `state.conversationWorkspaces` 中当前已注册的 workspace；已移除或未注册的历史 workspace 不参与。范围条件必须在 SQLite 的 FTS 排序和 `LIMIT` 之前生效，不能先从全局索引截断再在 Tauri 层过滤，否则范围外的命中会占用结果名额。

当前会话搜索字段为 task `title`、`description` 和 `authoring/requirement.md` 需求正文。后续聊天消息属于 `session_prompts_fts` 领域，当前不合并到该会话搜索入口。

查询文本按普通文本而非 FTS 运算表达式处理。以空白拆分的多个关键词使用 AND 语义；每个关键词均不少于 3 个 Unicode 字符时，通过 trigram FTS 检索，并使用 BM25 权重使标题命中优先于描述、需求正文。只要存在 1～2 字符关键词，就在当前 sidebar workspace 范围的 `tasks` 行内执行大小写归一后的字面包含匹配，以覆盖“你好”“随便”等 trigram 无法生成 token 的短查询。短查询仍在 workspace 范围和 `LIMIT` 约束内执行。

`matchPreview` 必须从实际命中的标题、描述或完整需求正文中生成，而不是固定返回需求开头。检索完成后由存储层统一定位首个命中关键词并生成稳定摘要：内容不超过摘要上限时完整展示，避免把短标题或短需求从单词中间切断；只有长文本才在关键词前保留最多 10 个字符，确保单行结果在截断前能够看到命中位置。会话搜索结果第二行展示该摘要，并由前端按当前查询关键词进行字面高亮；高亮使用无底色的高对比 `foreground` 文字和轻量下划线，确保亮色、深色主题均清晰可见。

workspace 项目 ID 必须统一通过 `GoldBandPaths::project_id` 生成，不能在 Tauri command 层重新实现路径转 ID。Windows 历史状态中可能存在 drive letter 大小写差异，workspace 解析按 ASCII 大小写不敏感匹配，并返回状态中已有的规范 ID，避免 FTS 已命中却在路由组装阶段被过滤。

### `search_acp_prompts`

全文搜索用户 prompt（FTS5 over `session_prompts_fts`）。返回 `PromptSearchResult`：

```jsonc
{
  "promptEventId": "...",
  "sessionId": "...",
  "promptId": "...",
  "timestamp": "...",
  "text": "...",
  "attemptPath": "...",
  "taskId": "...",
  "runId": "...",
  "roundId": "...",
  "nodeId": "...",
  "attemptId": "...",
  "sessionTitle": "..."
}
```

### `search_acp_sessions`

按标题模糊搜索会话（LIKE）。返回 `SessionSearchResult`：

```jsonc
{
  "sessionId": "...",
  "attemptPath": "...",
  "taskId": "...",
  "runId": "...",
  "roundId": "...",
  "nodeId": "...",
  "attemptId": "...",
  "title": "...",
  "status": "...",
  "createdAt": "...",
  "updatedAt": "..."
}
```

## 依赖

- `rusqlite = "0.34"`（`bundled` feature，自带 SQLite 编译，无需系统安装）
