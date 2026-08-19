# 工作空间 Project ID 统一与迁移技术方案

状态：已完成

日期：2026-08-19

## 1. 决策摘要

Gold Band 只保留一套 workspace 业务身份：`project_id`。

```text
normalized_workspace_path
  -> readable slug
  -> BLAKE3(normalized_workspace_path) 前 8 位小写十六进制
  -> project_id = {slug}--{hash}
```

项目运行目录统一为：

```text
~/.gold-band/projects/{project_id}/
```

`workspace_path` 继续保存当前位置，`name` 继续作为用户可编辑展示字段，`normalized_repo_root` 只参与 ID 生成、路径比较和 manifest 归属校验。三者都不是第二套业务 ID。现有 `workspace_key` 从 Runtime recovery 的结构、主键和接口中删除。

本方案包含旧项目目录、用户状态、`core.db` Runtime recovery 数据、项目 manifest、运行恢复文件中明确持久化的旧 runtime 绝对路径和搜索索引迁移。定时任务表与 Git linked-worktree 元数据修复不在本次范围内。

## 2. 根因与目标

### 2.1 根因

旧 `project_id` 只由可读路径 slug 生成。不同路径经过字符替换和截断后可能得到相同 slug，无法独立承担 workspace 数据隔离。与此同时，Runtime recovery 使用规范化路径形成 `workspace_key`，其它领域使用 `project_id`，造成同一 workspace 存在两套业务身份、主键和错误参数。

这是身份模型缺陷，不应通过在个别调用点补传路径或增加 fallback 修补。

### 2.2 目标

- 同一规范化 workspace 路径在相同身份配置下始终生成相同 `project_id`。
- 可读 slug 保留目录可辨识性，8 位哈希补足路径身份熵。
- `project_id` 成为目录、持久引用、路由、事件、缓存和恢复候选的唯一 workspace 身份。
- 旧用户目录和非定时任务数据一次性迁移，不静默覆盖或丢弃。
- 迁移中断后可以安全重放；完整成功后不再重复扫描和写盘。
- 不建立旧 ID 双读、长期 alias 表或 `workspace_key` 兼容层。

### 2.3 非目标

- 本阶段不修改 `scheduled-tasks.db` 的 schema、记录 `project_id` 或 definition JSON。
- 本阶段不执行 `git worktree move/repair`，不修复主仓库 `.git/worktrees/*/gitdir` 等 Git 元数据；项目父目录改名后，Gold Band 自身持久化的 `RunWorktreeState.path` 必须同步改写到新 runtime root。
- workspace 展示名称修改不触发 ID 变更或数据迁移。
- workspace 源目录被移动或重命名后的显式重新关联，不属于本次旧 ID 算法升级。

## 3. 配置契约

身份策略源参数统一存放在 `configs/app-config.toml`：

```toml
[projectIdentity]
maxLength = 80
hashHexLength = 8
separator = "--"
```

只配置三个独立源值。slug 最大长度必须在代码中计算，不能再保存一份 `70`：

```text
slugMaxLength = maxLength - separator.length - hashHexLength
              = 80 - 2 - 8
              = 70
```

后续实现新增 `ProjectIdentityConfig`，由 `ProjectAppConfig` 解析并在启动时校验：

- `maxLength` 必须大于 `separator.length + hashHexLength`。
- `hashHexLength` 必须大于 0 且不超过 BLAKE3 十六进制输出长度。
- `separator` 必须非空、只包含路径组件安全的 ASCII 字符。
- 配置非法属于构建或启动配置错误，不允许静默回退到另一组身份参数。

这些值虽然从统一 TOML 读取，但属于随应用版本发布的身份 schema，不是用户设置。修改任一值都必须提升 workspace identity migration 版本并提供新迁移。

## 4. Project ID 生成契约

### 4.1 路径规范化

生成入口只接受 workspace 路径，并统一执行：

1. 对存在的目录执行文件系统 canonicalize。
2. 将路径分隔符统一为 `/`。
3. 移除 Windows extended-length 前缀。
4. Windows 下统一 ASCII 小写，消除 drive letter 和大小写差异。
5. 输出 `normalized_workspace_path`，供哈希、比较和 manifest 校验共同使用。

禁止 slug 和哈希分别使用两套规范化结果。

### 4.2 可读 slug

- 从 `normalized_workspace_path` 生成 ASCII slug。
- ASCII 字母、数字、`.`、`_` 保留；其它连续字符折叠为一个 `-`。
- 去除首尾 `-`；空结果使用 `root`。
- 超过计算出的 `slugMaxLength` 时保留尾部，优先保留最终仓库目录名。

slug 只提供可读性，不单独承担唯一性。

### 4.3 哈希与最终格式

```text
digest = blake3(normalized_workspace_path UTF-8 bytes)
hash = lowercase_hex(digest)[0..hashHexLength]
project_id = slug + separator + hash
```

这里是 8 位十六进制短哈希，不称为 UUID，也不能使用随机 UUID v4、`DefaultHasher` 或进程种子。BLAKE3 算法、输入编码和截取方向属于持久化契约，必须用固定向量测试锁定。

稳定边界：

- 同一规范化绝对路径重复计算、应用重启后结果相同。
- Windows 路径大小写和分隔符差异不改变结果。
- workspace 展示名称变化不改变结果。
- 绝对路径变化或在另一台机器使用不同路径时，结果允许变化。

## 5. 目标数据结构

### 5.1 Workspace 注册状态

```rust
ConversationWorkspaceEntry {
    project_id: String,     // 唯一业务身份
    workspace_path: String, // 当前磁盘 locator
    name: String,           // 可编辑展示名称
    added_at: String,
}
```

注册时分别校验规范化路径和最终 `project_id`：

- 相同路径已存在：返回 `workspace.already-exists`。
- 相同 `project_id` 指向不同规范化路径：返回 `workspace.project-id-collision`。
- 目标目录 manifest 与当前路径不匹配：返回 `workspace.manifest-mismatch`。

展示名称重名限制属于产品规则，不能替代最终 ID 和 manifest 归属校验。

### 5.2 Project manifest

```rust
ProjectManifest {
    version: String,
    project_id: String,
    repo_root: String,
    normalized_repo_root: String,
}
```

创建或打开项目运行目录前必须读取并验证 manifest。已有目录缺少 manifest、manifest 损坏或归属不一致时禁止继续写入，不能静默覆盖。manifest 使用临时文件加原子替换写入。桌面上下文启动也属于该边界；manifest 读取、创建或原子提交发生 I/O 失败时必须返回失败，不得降级为成功后继续初始化 workspace。

### 5.3 Runtime recovery

删除 `RuntimeRecoveryCandidate.workspace_key`，目标表为：

```sql
CREATE TABLE runtime_recovery_candidates (
    project_id          TEXT NOT NULL,
    workspace_path      TEXT NOT NULL,
    task_id             TEXT NOT NULL,
    run_id              TEXT NOT NULL,
    candidate_token     TEXT NOT NULL,
    runtime_instance_id TEXT NOT NULL,
    registered_at_ms    INTEGER NOT NULL,
    PRIMARY KEY (project_id, task_id, run_id)
);
```

进程内 `RuntimeRunKey`、blocked workspace 集合、登记、条件删除和恢复查询统一使用 `project_id + task_id + run_id`。`workspace_path` 只用于恢复时解析源目录，并必须重新计算 `project_id`、校验 manifest 后才能访问运行数据。

### 5.4 搜索索引

`gold-band.db` 是可从文件事实源重建的投影。`tasks.task_path`、session attempt path 等包含旧 runtime root，目录迁移后不逐行修补，而是在事务内清空旧索引数据，再复用现有磁盘 backfill 从新目录完整重建。身份迁移必须等待本轮 backfill 完成后才能提交最终版本；不能只启动后台回填就宣告迁移成功，因为进程中断可能留下非空但不完整的索引。

## 6. 迁移范围

### 6.1 纳入迁移

| 数据 | 迁移动作 |
|---|---|
| `projects/{旧project_id}` | 移动为 `projects/{新project_id}` |
| `project.json` | 重写新 `project_id` 和当前路径事实 |
| `conversation_workspaces` | 根据 `workspace_path` 重算 `project_id` |
| `last_conversation_workspace` | 按旧 ID 到新 ID 映射替换 |
| `conversation_pins` | 替换 `project_id`，按新组合身份去重 |
| `conversation_run_modes` | 将 map key 替换为新 `project_id` |
| `core.db.runtime_recovery_candidates` | 删除 `workspace_key`，按 `workspace_path` 重算新 `project_id` |
| `run.json.worktree.path` | 仅当明确位于旧 runtime root 下时替换根前缀 |
| `run.json.last_executed_node.attemptDir` | 仅当明确位于旧 runtime root 下时替换根前缀 |
| `worker-ref.json.continue_ref.cwd / snapshotFile` | 仅当明确位于旧 runtime root 下时替换根前缀 |
| `acp.session.json.cwd / acp.snapshot.json.cwd` | 仅当明确位于旧 runtime root 下时替换根前缀 |
| `acp.turn-file-mutations.jsonl.logicalPath` | 逐条解析，仅替换旧 runtime root 前缀 |
| `turn-file-change-sets/turn-files-*.json` | 改写 `changes[].logicalPath / previousLogicalPath` 的旧 runtime root 前缀 |
| `gold-band.db` 搜索数据 | 清空旧投影并从新目录重建 |

普通 Task、Run、Attempt 的层级编号、业务身份和生命周期状态保持不变。迁移器只遍历当前 workspace runtime 目录并按已知文件名、已知 JSON 字段结构改写 executable locator，禁止对全部 JSON 做无类型字符串替换。`acp.raw.jsonl`、`acp.timeline.jsonl` 和 diagnostics 是历史审计/展示事实，保持字节与历史内容不变，不参与会话恢复。

### 6.2 定时任务边界

项目父目录移动时，`scheduled-tasks.db` 和 `scheduled-tasks/` 会作为普通文件原样移动，但本阶段不打开、不改写其表和 definition JSON。因此本阶段不承诺历史定时任务可按新 `project_id` 工作。

开发可以拆分提交；如果已有用户依赖定时任务，发布版本必须在后续全局 Scheduler 数据迁移完成后再交付，不能把该中间状态作为完整升级发布。

### 6.3 Worktree 边界

本阶段不执行 `git worktree move/repair`，不重写主仓库 `.git/worktrees/*/gitdir` 等 Git 自身管理的绝对回链。项目父目录整体 rename 后，迁移器会改写 `run.json.worktree.path` 和 ACP `continue_ref.cwd`，确保 Gold Band 不再请求已消失的旧目录；这不等于承诺所有历史 linked worktree 的 Git 管理操作均已 repair。普通 Task/Run 历史不因此丢失，该边界必须在验收记录中明确。

## 7. 一次性迁移与幂等设计

### 7.1 完成版本

复用 `core.db.core_schema`，增加独立组件版本：

```text
component = "workspace_identity"
version = 2
```

只有目录、manifest、用户状态、Runtime recovery schema/data、全部 executable locator 和搜索旧投影都完成后，才在最后一个短事务中写入该版本。v1 只完成了目录与部分 `cwd` 迁移；版本低于 2 时必须补跑 locator 改写。版本已经达到 2 时，启动不再枚举旧目录或改写状态。

`StateConfig.state_schema_version` 同步提升并负责 JSON 字段引用迁移；它不是跨文件系统迁移的唯一完成标记。跨存储迁移是否完整以 `core_schema.workspace_identity` 为准。

### 7.2 为什么中断后不会重复

迁移使用“最终版本门闩 + 每一步幂等”的双重保证：

1. 成功后命中版本门闩，不再执行。
2. 成功前即使进程中断，下次也重新生成同一份确定性 `old_project_id -> new_project_id` 计划；扫描到合法 manifest 时，以 manifest 所在实际目录作为旧数据来源，不能因原 workspace 已删除而从失效路径重新猜测旧目录名。
3. 每一步根据当前事实判断，不假设上次完全没执行。
4. 所有集合迁移使用替换或主键 upsert，不做无条件 append。
5. 完成版本最后写入，不能先标记成功再搬数据。

目录状态判定矩阵：

| 旧目录 | 新目录 | 处理 |
|---|---|---|
| 存在 | 不存在 | 校验归属后执行同卷 rename |
| 不存在 | 存在且 manifest 路径匹配 | 视为目录步骤已完成，继续后续步骤 |
| 存在 | 存在 | 禁止合并或覆盖，返回结构化冲突 |
| 不存在 | 不存在 | 已注册但从未产生 runtime 数据时只迁移状态引用，后续正常 provision；未被任何状态或 manifest 识别的孤立目录保持原样并跳过 |
| 任意 | 新目录 manifest 指向其它路径 | 返回 collision，不写入 |

各存储的重复执行语义：

- manifest：以相同权威内容原子替换，重复写结果一致。
- StateConfig：从 `workspace_path` 重算并赋值，用新组合键去重，不追加副本。
- `core.db`：在 `BEGIN IMMEDIATE` 事务中建立新表、按新复合主键插入、校验数量后交换表；事务回滚不会留下半张表。
- 搜索索引：每次未完成迁移都先清空包括上次部分 backfill 在内的可重建投影，再同步调用现有 backfill 完整重建；只有 backfill 成功后才允许提交最终版本。
- 目录：只允许上述四态转换，不递归合并两个目录。

因此，进程即使在 rename 后、StateConfig 保存后或 SQLite 提交前退出，下次都能从磁盘与数据库事实继续向前收敛，而不会再次复制目录、重复 pin 或产生重复 recovery candidate。

### 7.3 启动顺序

```text
读取 app identity config
  -> 打开最小 StateConfig/core.db
  -> 检查 workspace_identity migration version
  -> 构建并全量预检查迁移计划
  -> 迁移目录与 manifest
  -> 原子保存 StateConfig
  -> 事务迁移 core.db recovery table
  -> 清空搜索旧投影
  -> 同步完成搜索 backfill
  -> 写入 workspace_identity=2
  -> 初始化正常搜索服务
  -> 执行 Runtime recovery
  -> 启动 Scheduler
  -> 开放正常 workspace 命令
```

迁移和 Runtime recovery、Scheduler、新 Run admission 不并发。目录和 SQLite 临界区内不执行 provider、网络或其它长任务。

## 8. 迁移计划与冲突检查

迁移来源按优先级为：

1. 项目目录内有效 `project.json.normalized_repo_root`。
2. `conversation_workspaces.workspace_path` 和 recent workspace 路径按旧算法得到的目录名。
3. 无 manifest 且不被任何已知 workspace 引用的目录不猜测归属，保持原样并跳过，不阻断其它 workspace 迁移和应用启动。

正式写入前先构建完整计划并检查：

- 一个旧目录不能对应多个规范化路径。
- 多个旧目录不能映射到同一个新 `project_id`。
- 新目录不能属于其它规范化路径。
- 相同旧 slug 已经承载多个 workspace 时不能自动拆分历史数据。
- 配置引用、manifest 和磁盘目录必须能够形成唯一映射。

预检查失败时不开始该批迁移。错误使用稳定 code 和结构化参数，后端不生成对客文案：

- `workspace.identity-migration-conflict`
- `workspace.identity-migration-source-missing`
- `workspace.identity-migration-manifest-invalid`
- `workspace.identity-migration-runtime-state-invalid`
- `workspace.project-id-collision`

## 9. 实现接口

计划新增或收敛的核心接口：

```rust
ProjectIdentityConfig::validate()
storage::project_id_for_workspace(path)
GoldBandPaths::validate_project_manifest()
WorkspaceIdentityMigrator::plan(...)
WorkspaceIdentityMigrator::execute(...)
WorkspaceIdentityMigrationReport
```

`WorkspaceIdentityMigrationReport` 返回逐 workspace 的 `migrated/already-migrated/unresolved/conflict` 结构化结果和最终版本，不以日志文本充当业务结果。

前端和 Tauri 业务命令继续使用既有 `projectId` 字段，不新增 `workspaceKey`。升级后路由值改变，由已迁移的 workspace canonical state 重新投影；不保留旧 route alias。

## 10. 实施顺序

1. 接入并校验 `ProjectIdentityConfig`，用固定向量锁定路径规范化和 ID 输出。
2. 统一 `GoldBandPaths` 的 normalized path、slug、哈希和长度计算入口。
3. 增加 manifest 读取与归属校验，禁止静默写入错误目录。
4. 将 Runtime recovery 从 `workspace_key` 改为 `project_id`，升级 `core.db` schema。
5. 实现只在启动早期运行的 workspace identity migrator。
6. 迁移 StateConfig 和明确持久化的 runtime 绝对路径。
7. 清空并同步复用现有搜索 backfill 完整重建索引。
8. 接入完成版本门闩，删除运行期旧 ID fallback 和 alias 消费路径。
9. 更新产品设计、MVP 开发记录并完成接口级回归。

## 11. 验收与回归测试

### 11.1 ID 契约

- 固定输入得到固定 `slug--8hex` 输出。
- 同一路径重复调用及进程重启后结果一致。
- Windows drive letter 大小写和 `/`、`\\` 差异不改变结果。
- 长路径的完整 ID 不超过 80，slug 上限由配置计算为 70。
- workspace 展示名称变化不改变 `project_id`。
- manifest 中相同 ID、不同规范化路径被拒绝。

### 11.2 迁移接口

- 普通项目目录及 Task/Run/Attempt/附件完整移动。
- `run.json`、`worker-ref.json`、ACP session/snapshot 和文件变更记录中的 executable locator 全部改写到新 runtime root；raw/timeline/diagnostics 历史记录保持不变。
- StateConfig 四类 project 引用全部更新且无重复。
- `workspace_key` 从 `core.db` schema、Rust DTO、内存 key 和错误参数中消失。
- recovery candidate 数量、token 和 locator 在 schema 迁移前后保持一致。
- 旧搜索路径全部消失，新路径 backfill 后可搜索和导航。
- 在目录 rename、StateConfig 保存、core.db 事务、搜索清空和部分 backfill 后分别模拟中断；再次执行均先移除部分投影并收敛到相同最终状态。
- 完整迁移执行第二次只读取版本，不枚举或写入 workspace 数据。
- 目标冲突和已知 workspace 的损坏/缺失 manifest 不覆盖任何目录；无 manifest 且无状态归属的异常目录原样保留并跳过。
- `scheduled-tasks.db` 字节保持原样；其表功能不计入本阶段验收。
- `RunWorktreeState.path` 与 ACP worktree `cwd` 不再引用旧 root；Git linked-worktree 元数据 repair 和完整 Git 管理操作不计入本阶段验收，并在报告中明确该边界。

### 11.3 验证命令

实现阶段至少执行相关 Rust 单元测试、core/storage/desktop 接口测试、两个 Rust crate check、格式检查和 diff 检查。涉及启动命令链路后再执行实际 EXE 启动迁移验证；本方案不涉及 UI 样式，不需要浏览器视觉验收。

## 12. 性能与过度设计评审

### 12.1 性能

- 日常 ID 生成只对一个短路径执行一次规范化和 BLAKE3，成本可忽略。
- 目录同卷 rename 主要是文件系统元数据操作，不按 Task/Run 文件体积复制。
- Runtime recovery 数据迁移受现有 4096 条上限约束，单个 SQLite 短事务完成。
- 搜索重建与 Task/Session 数量线性相关，属于一次性投影恢复；先清除旧路径和上次可能留下的部分结果，再在最终版本提交前完成全量 backfill，避免永久保留不完整索引。
- 不增加常驻扫描、轮询、缓存、队列、N+1 请求或 UI 订阅。

### 12.2 过度设计

方案复用已有 BLAKE3、`configs/app-config.toml`、`StateConfig.state_schema_version`、`core_schema`、原子 JSON 写入、SQLite transaction 和搜索 backfill。不新增第二套 workspace aggregate、长期 alias 表、双写、双读或独立迁移数据库。

独立 `workspace_identity` 版本是跨目录、JSON 和两个 SQLite 存储无法使用单一事务时所需的最小完成门闩；每一步幂等用于解决门闩写入前崩溃，不是为假设性并发增加的状态机。

## 13. 实施结果

- `project_id` 已统一为 `{最多 70 位 slug}--{8 位 BLAKE3}`，配置源为 `configs/app-config.toml [projectIdentity]`，固定向量为 `d:/projects/example-app -> d-projects-example-app--3d4964d2`。
- Runtime recovery 与 ACP command catalog 已删除运行期 `workspace_key`；`core.db` schema v2 使用 `project_id + task_id + run_id`，旧 v1 表在单个 SQLite 事务中重建。
- 迁移器已覆盖“workspace 已删除但合法 manifest 与旧项目目录仍保留”的情况，并将迁移测试的 HOME 完全隔离，避免测试写入真实用户 `state.json`。
- 无 manifest 且不被 StateConfig 识别的异常历史目录不再阻断启动，迁移器保持目录原样并跳过，不猜测归属或并入其它 workspace。
- 桌面启动在正常 manifest provision、Runtime recovery 和 Scheduler 之前执行 `WorkspaceIdentityMigrator`。迁移器全量预检查后移动旧目录，重写 manifest、StateConfig，以及 `run.json`、`worker-ref.json`、ACP session/snapshot 和文件变更记录中的明确 executable locator，同步清空并重建搜索投影，最后写入 `core_schema.workspace_identity=2`。已完成 v1 的机器会补跑 locator 修复；raw/timeline/diagnostics 历史记录不改写。
- workspace 注册、同步、切换、Runtime recovery 与 Scheduler 注册均执行严格 manifest 归属校验；缺失、损坏或归属不一致不再静默覆盖或继续写入。迁移完成后 workspace 解析只接受持久化 canonical `project_id` 精确匹配，不保留大小写 alias 或路径重算 fallback。
- 桌面上下文初始化已删除 manifest I/O 失败的读取侧吞错分支；当前 workspace 的 manifest 无法读取、创建或原子提交时直接阻止初始化，并由定向单元测试固定 I/O 与完整性失败契约。
- 定向测试已固定配置派生、固定哈希向量、长度截断、manifest mismatch、core v1→v2、recovery token fencing、目录迁移、状态映射、rename 后 v1 marker 补跑、全部 executable locator、raw 审计不变、搜索重建、Scheduler DB 字节不变和完成 marker 二次短路。
- 按本次验收约定仅执行 Rust 格式检查、两个 crate 构建和相关定向测试；未执行全量回归与前端验证。Git worktree 修复及 Scheduler 表/definition schema 迁移仍明确不在本方案范围内。
