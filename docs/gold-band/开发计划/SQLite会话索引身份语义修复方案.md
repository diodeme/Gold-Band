# SQLite 会话索引身份语义修复方案

## 1. 问题与根因

`gold-band.db` 是可从文件重建的辅助搜索索引。旧 schema 将 `acp.snapshot.json.adapterId` 写入 `sessions.session_id` 和 `session_prompts.session_id`，导致 `npx`、`kimi`、`opencode` 等 adapter 启动命令通过搜索接口被错误序列化为 `sessionId`。

这不是单条数据损坏，而是 session identity 与 adapter identity 混用的设计缺陷。真实 ACP session ID 已由 `AcpSessionMetadata.session_id` 表达，attempt 的稳定索引身份仍是 `attempt_path`。

## 2. 依赖与最佳实践评估

- 继续复用现有 `rusqlite`、SQLite schema version、派生表重建和磁盘回填机制，不增加迁移库或第二套索引。
- 按关系模型拆分不同领域字段：真实会话身份使用 `session_id`，adapter 身份使用 `adapter_id`。
- session 尚未建立或旧文件缺失真实 ID 时使用 SQL `NULL`，不使用空字符串、adapter ID、attempt ID 或 timeline 扫描伪造身份。
- SQLite 仍不是 session canonical state；文件写入成功后再刷新索引。

## 3. 数据与接口方案

schema v4：

1. `sessions.session_id` 改为可空，来源固定为 `snapshot.session_id`。
2. `sessions.adapter_id` 新增为非空字段，来源固定为 `snapshot.adapter_id`。
3. `session_prompts.session_id` 改为可空；重复索引同一 prompt 时同步更新 session identity。
4. `SessionSearchResult.sessionId` 与 `PromptSearchResult.sessionId` 改为可空。
5. `SessionSearchResult` 增加 `adapterId`，避免调用方再从 `sessionId` 猜测 adapter。

## 4. 升级与写入顺序

1. 启动打开 SQLite 后检测 `PRAGMA user_version`。
2. 任意旧版本都破坏式删除 `sessions`、`session_prompts` 及其 FTS/trigger；`tasks` 表继续保留。
3. 创建 v4 会话索引表并提交 `user_version = 4`。
4. 空会话表触发现有后台 backfill，从 `acp.snapshot.json` 和 `acp.timeline.jsonl` 重建。
5. 新写入继续使用事务原子 upsert；DB 失败不回滚文件事实源。

开发阶段不保留旧错误语义兼容层，也不原地信任或转换 `npx` 等历史值。

## 5. 验收

- snapshot 同时包含 `sessionId = session-real-123` 与 `adapterId = npx` 时：
  - session/prompt 搜索结果的 `sessionId` 为 `session-real-123`；
  - session 搜索结果的 `adapterId` 为 `npx`。
- 后续 snapshot 缺失 `sessionId` 时，重复索引把 session/prompt 搜索结果收敛为 `sessionId = null`，不保留旧 ID。
- v3 数据库升级后旧会话索引行被删除，schema 使用可空 `session_id` 和独立 `adapter_id`，已有 task 行不丢失。

接口级回归测试：`tests/sqlite_session_identity.rs`。

## 6. 方案自评审

### 过度设计

未新增依赖、持久化事实源、状态机、缓存或兼容分支。新增一个字段只用于保留原本被错误占用的 adapter 语义，与实际数据和接口需求匹配。

### 性能影响

- 每个 session 行增加一个短文本字段，数量与 attempt 数量同阶，空间增长有界。
- 单次 upsert、标题查询和 prompt FTS 的算法复杂度、查询次数与锁范围不变。
- schema 升级触发一次既有后台全量回填；不增加运行期轮询、timeline 额外扫描、N+1 请求、缓存或队列。
- 风险较低，无需新增 benchmark；以 schema 重建测试、接口级回归测试和 `cargo check` 验收。

## 7. 完成记录

2026-08-19 已完成 schema v4、写入映射、搜索 DTO、升级重建与接口级测试。定向集成测试 2 项通过；全量 lib unit test 受工作区既有 `src/config/mod.rs` 测试编译错误阻塞，与本修复无关。
