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
文件写入成功 → 异步写 SQLite 索引 → 失败侧：重试3次，每次重新读文件 → 仍失败则 trace log 丢弃
```

- 文件永远先写，SQLite 后写。
- 写入非阻塞：所有 DB 操作通过 `spawn_blocking` 或 `std::thread::spawn`，不在主线程执行。
- `Mutex<Connection>` 仅持有事务期间，不在文件 I/O 期间持有。
- 重试间隔：200ms → 500ms → 1500ms，每次重新读取最新文件内容。

## 表设计

### `tasks`

存储任务元信息，用于跨项目检索 task。

| 列 | 类型 | 来源 | 说明 |
|---|---|---|---|
| `task_id` | TEXT PK | task 目录名 | 全局唯一任务 ID |
| `task_path` | TEXT | 文件系统 | task 目录路径 |
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
| `create_task` | task 元信息 + requirement | `spawn_blocking` |
| `send_acp_prompt` 完成后 | session + prompts | `spawn_blocking` |
| `respond_acp_permission` 完成后 | session（重新读取最新文件） | `spawn_blocking` |
| `cancel_acp_session` 完成后 | session（重新读取最新文件） | `spawn_blocking` |
| orchestrator 节点执行完成后 | session + prompts | `std::thread::spawn` |

所有写入均为 fire-and-forget。

## 搜索接口

### `search_tasks`

搜索 task 的标题、描述、需求内容（FTS5 over `tasks_fts`）。返回 `TaskSearchResult`：

```jsonc
{
  "taskId": "...",
  "taskPath": "...",
  "title": "...",
  "description": "...",
  "requirementPreview": "前500字符..."
}
```

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
