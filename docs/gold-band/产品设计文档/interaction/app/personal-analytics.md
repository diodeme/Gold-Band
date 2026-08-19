# 个人数据分析

## 1. 状态与范围

- 产品状态：已实现 SQLite 派生索引、自动增量同步、日期范围确定性报告、章节导航和独立 Agent 洞察生命周期。
- 用户入口：“帮助”菜单中与“用户反馈”同级的“个人数据分析”。
- 用户流程：从帮助菜单直接进入独立原生统计页面；页面自动同步 SQLite 索引并展示确定性报告，用户可切换日期范围，或显式选择可用 Agent 生成当前范围的分章节洞察。
- 范围：只实现个人数据分析专用内置能力，不建设通用 `SkillSource::BuiltIn` 平台。
- 报告：只接受版本化 JSON DTO，由 React 原生页面渲染；不执行 Agent 生成的 HTML。

## 2. 根因与设计原则

个人分析不能等同于“把 `.maling/projects` 整个目录交给 Agent”。当前本机样本为 4 个项目、3334 个文件、374,080,616 bytes，其中 `acp.raw.jsonl` 占 253,540,256 bytes，并与规范化 timeline 存在协议层重复。直接扫描会造成无界上下文、重复统计、隐私内容扩散和指标口径漂移。

采用以下分层：

1. 客户端以 canonical 文件为唯一权威事实源维护 SQLite 派生索引，并从索引生成确定性日期范围投影。
2. 客户端生成 evidence locator 授权清单和内联有界内容的语义批次。
3. 所选 Agent 只接收三个客户端投影 ACP resource 附件，不获得原始项目目录读取权限，并输出版本化洞察扩展。
4. 客户端校验洞察扩展；仅在 schema 不合法时执行一次结构修复。
5. 客户端把确定性报告与校验后的洞察按章节组装，原生页面只消费有界 DTO。

“全部”范围是默认报告口径；具体日期范围按本地自然日起止日包含归属。索引首次全量解析后，页面打开或范围切换只触发后台增量同步，同步期间保留当前可用报告。

## 3. 数据领域与事实源

### 3.1 业务实体

| 实体 | 归属 | 权威事实 | 生命周期 |
| --- | --- | --- | --- |
| `AnalyticsIndex` | analytics | 8 张物理表、4 个逻辑视图、fingerprint、indexRevision | 可删除重建的派生投影，无源文件变化时不推进版本 |
| `AnalyticsOperation` | application | 确定性同步 operationId、status、revision、progress、indexRevision、reportId | queued → scanning → completed/failed/cancelled |
| `AgentInsightOperation` | inference | 洞察 operationId、range、schemaVersion、indexRevision、agentType、status | queued → analyzing → validating-report → completed/failed/cancelled |
| `AnalyticsProjection` | analytics | 日期范围聚合、coverage、metric definitions、evidence locators | 由 indexRevision 标识的只读派生物 |
| `ContentManifest` | analytics authorization | 本次语义样本的 evidence locator 与明确排除源 | locator 只用于证据引用，不代表原文件读取权限 |
| `SemanticBatchManifest` | analytics authorization | 批次、预算、样本 coverage、允许内容 locator | 有界批次，完成后不改变确定性统计 |
| `AnalyticsNarrative` | analytics inference | 按质量、效率、Token、上下文与技能分区的 insight | schema 校验通过后与投影合并 |
| `AnalyticsReport` | analytics | schemaVersion、sourceCoverage、metrics、insights、warnings | 校验通过后成为页面可读报告 |

`AnalyticsProjection` 和 `AnalyticsReport` 都是投影，不得修改或覆盖 `.maling/projects` canonical 数据。

### 3.2 canonical 来源优先级

索引器优先使用现有 typed model/parser 读取：

1. 身份与会话：`project.json`、`task.json`、`conversation.json`。
2. Workflow：`run.json`、`round.json`、`node.json`。
3. Direct turn 与 usage：`turn.json`、`acp.prompt-usage.jsonl`。
4. ACP 规范化内容：`acp.snapshot.json`、`acp.timeline.jsonl`。
5. AUTO：`dynamic-run.json`、`graph.json`。
6. 可观测事实：`observability.snapshot.json`、`events.jsonl`。

默认排除 `acp.raw.jsonl`、`doctor/`、诊断日志、数据库/WAL、ZIP、PID、class 和二进制文件。扫描器在 canonical root 内拒绝 symlink/reparse point 和越界路径；Agent 只处理已内联进语义批次的文本，不按 locator 回读原文件。

历史 `turn.json` 覆盖不足时，Direct 指标必须展示 unknown/coverage warning；不得从 `availableCommands` 推断 Skill 调用，也不得用包含完整命令或路径的 fallback title 作为工具名称。

## 4. 指标契约

页面使用以下三个准确名称：

- Direct 回复完成率：正常完成的 Direct turn / 符合口径的已开始 Direct turn。
- Workflow run 终局成功率：`outcome=success` 的 Workflow run / 符合口径的已开始 Workflow run。
- AUTO outer run 终局成功率：`outcome=success` 的 AUTO outer run / 符合口径的已开始 AUTO outer run。

失败、取消、未达终局、仍在进行和未知数据分别展示，不能从分母覆盖说明中消失。所有成功/完成指标同时展示终局覆盖率和样本量。

累计执行耗时以节点 attempt 的 `acp.snapshot.json.timing.sessionElapsedSeconds` 为唯一事实源，并使用 project/task/run/node/attempt 完整 locator 关联。任务和终局 Run 累计其全部节点 attempt，包含重试；AUTO 并行节点求和表示累计 Agent 执行时间，不代表端到端等待时长。缺失或无效 snapshot timing 按 `0` 进入累计值、平均值和排行，并通过 `activeDurationZeroFilledCount` 与 `analytics.active-duration-zero-filled` warning 公开补零数量。Run 的 RFC 3339/历史 epoch 时间只用于历史范围、最近活动和排序，不参与执行耗时计算，也不从相邻时间戳推断耗时。

“最近任务”只展示 Workflow 和 AUTO：Workflow 使用最新 run 的 `status + outcome`；AUTO 只使用 outer run 的 `status + outcome`，不重复统计 dynamic node 或 child workflow。Direct 不进入最近任务，但继续参与 Direct 回复完成率等明确支持的整体指标。

不得展示“用户任务成功率”。当前数据只能证明回复或 runtime 是否到达相应终局，不能证明用户验收。没有 canonical 证据时也不展示 AI 代码留存/覆盖、真实金额、Skill 使用次数或 provider/model 因果贡献。

## 5. 固定提示词协议

提示词采用现有 MiniJinja 严格渲染，不新增 Skill 包或依赖：

| Prompt | 中文 | 英文 | 职责 |
| --- | --- | --- | --- |
| system | `src/prompts/zh-CN/personal-analytics/system.md` | `src/prompts/en/personal-analytics/system.md` | 角色、信任边界、指标、证据和输出 schema |
| user | `src/prompts/zh-CN/personal-analytics/user.md` | `src/prompts/en/personal-analytics/user.md` | 本次 operation、路径、水位和覆盖摘要 |
| repair system | `src/prompts/zh-CN/personal-analytics/repair_system.md` | `src/prompts/en/personal-analytics/repair_system.md` | 仅修复结构错误，禁止重算或新增事实 |
| repair user | `src/prompts/zh-CN/personal-analytics/repair_user.md` | `src/prompts/en/personal-analytics/repair_user.md` | 无效报告、校验错误和目标 schema |

分析 user prompt 变量：`operation_id`、`report_schema_version`、`source_watermark`、`index_revision`、`date_range`、`projection_path`、`content_manifest_path`、`semantic_batch_manifest_path`、`coverage_summary`。system prompt 另接收 `report_schema`。

repair user prompt 变量：`operation_id`、`invalid_report_path`、`validation_errors`、`report_schema`。

Agent 最终只能输出 `PersonalAnalyticsNarrative`，包含 `schemaVersion + insights`；每条 insight 必须归入 `quality`、`efficiency`、`token-usage`、`context-and-skills` 之一。Agent 不输出确定性统计、最近任务或排行榜。洞察 JSON 从最新有界 ACP 消息按稳定身份契约提取。首次失败使用 repair prompt 重试一次；再次失败以 `analytics.report-invalid` 结束。repair 只修字段形状、类型、枚举或额外字段，不能重新读取数据、计算指标或添加洞察。

## 6. 原生页面交互

1. 点击“个人数据分析”直接导航到 `/chat/personal-analytics`；UI mode、主模块、页面状态和路由在同一导航事务中更新，不经过 Dialog。
2. 页面标题操作区提供全部、今天、近 7 天、近 30 天和自定义范围；自定义范围使用两个原生日期输入。
3. 桌面宽度使用左侧章节导航和右侧报告内容；窄宽度切换为横向吸顶导航，当前章节由 `IntersectionObserver` 标记。
4. 标题操作区使用 shadcn `Select` 列出可用 Agent，并只服务“生成 Agent 洞察”；确定性同步不依赖 Agent。
5. 同步期间保留当前可用报告并展示进度；洞察运行中锁定 Agent 选择并禁止重复启动，失败或取消不影响统计。
6. 页面使用现有 shadcn/ui 和 Tailwind 组件，参考增强版 HTML 的信息架构，不复制其营销式视觉或执行其中代码。

报告使用纵向原生分区，主要段落固定为：

1. 使用概览：规模、三个准确率、累计 Token、平均终局累计执行耗时和历史范围。
2. 最近任务：最近 10 条 Workflow/AUTO 任务，同时展示累计执行耗时和 Token。
3. 质量：纠错重入率、重试后恢复、失败/取消/暂停信号和对应洞察。
4. 效率：终局累计执行耗时、暂停恢复、TOP10 累计执行耗时任务、节点累计执行耗时和对应洞察；耗时 TOP10 同时展示 Token。
5. Token 消耗：输入/输出/缓存、TOP10 Token 任务和对应洞察；不展示真实金额。
6. 上下文与技能使用：工具、Agent、权限、用户补充请求和有显式 invocation 证据的 Skill。

报告内所有 Token 数值使用 K/M 紧凑格式，避免同一页面混用原始整数和缩写。

数据覆盖作为末尾固定区块。桌面宽度使用 shadcn Table；窄窗口切换为紧凑列表，不依赖横向滚动。

## 7. 性能与数据完整性

- 首次索引在 Rust blocking pool 中流式解析全部 eligible canonical 文件；后续按 `path + size + mtime + type` fingerprint 增量解析新增、变化和损坏文件，并删除消失源的派生事实。
- 日期切换只执行 SQLite 聚合查询，不重新解析历史文件，也不向 React 传递全历史明细。
- 不把完整 timeline、附件或 prompt 集合加载进内存，不在 React 计算全历史聚合。
- 最近任务和排行榜最多各 10 条，节点、工具、Agent、Skill 聚合最多各 12 条；页面 DTO 保持有界。
- 单项损坏、超大或未知版本只进入 coverage warning，不触发全量失败或无界重试。
- operation 使用稳定 ID 和单调 revision；迟到进度或失败不能覆盖新状态和终态。前端合并同范围报告时比较 `indexRevision`，低版本报告不能覆盖高版本报告。

SQLite 是唯一派生索引存储，始终可删除重建；无源文件变化时不推进 `indexRevision`，避免可用洞察被无效失效。

2026-08-19 release 同轮复测：SQLite 首次全量索引约 `19.330s`，既有投影路径约 `17.600s`；增量同步约 `9.758s`，只重解析测试期间变化的 `1` 个文件；日期范围聚合查询约 `64ms`。范围查询和增量收益达标，但首次索引比历史 `6.032s` 门槛和当前可比投影路径分别慢约 `13.298s`、`1.730s`；该差距保持未关闭，后续必须继续定位解析、SQLite 写入与磁盘冷/热缓存成本。

## 8. 验收边界

- 中英文模板结构一致，所有变量在 MiniJinja strict 模式下可渲染，缺失变量会失败。
- 三个指标名称、证据 locator、样本量、置信度、全历史/语义覆盖边界在两种语言中一致。
- Agent 无法从提示词获得扫描 `.maling/projects` 或读取清单外路径的权限。
- 无效报告只修复一次，修复过程不新增无证据事实。
- Rust 公开投影接口测试、前端 operation revision / 报告 indexRevision 合并测试和页面启动交互测试已固化；页面测试覆盖无可用 Agent、active operation、防重复提交、accepted 快照合并和长名称收缩约束，Tauri operation 模块还包含冲突与终态防迟到覆盖测试。
- `/chat/personal-analytics` 已完成 1280px/720px、浅色/深色 deep-link 验证；八个章节导航按钮均可点击滚动，当前章节状态随可视区更新；自定义开始/结束日期使用原生日期输入，合法范围可执行同步与 Agent 洞察，非法或缺失范围展示错误并禁用 Agent 洞察。页面选择器、启动按钮、选择器弹层和 Help 菜单直达均无 Dialog、重叠或页面横向溢出；720px 下章节导航自身允许内部横向滚动，报告区不产生横向滚动。

## 9. 2026-08-19 交互与数据生命周期优化实现

本节描述当前 release 行为。`.maling/projects` 仍是唯一权威事实源；SQLite 索引可删除重建，确定性统计与 Agent 洞察生命周期完全解耦。

### 9.1 章节导航

报告页在桌面宽度使用“左侧章节导航 + 右侧报告内容”布局。导航项固定为：使用概览、最近任务、终局可靠性、质量、效率、Token 消耗、上下文与技能使用、数据覆盖。点击导航项滚动到对应章节；当前章节通过 `IntersectionObserver` 标记，不监听高频 scroll 事件。窄窗口下导航切换为横向吸顶按钮组，保留同样的跳转能力，不制造横向报告滚动。导航复用 shadcn Button、ScrollArea 和 Tailwind token，不引入新基础组件。

### 9.2 SQLite 分析索引与日期筛选

`.maling/projects` canonical 文件仍是唯一权威事实源；SQLite 是可重建的分析投影索引。索引扩展现有 `gold-band.sqlite`，不创建第二个数据库，不把 SQLite 写回业务文件。

首次生成时执行全量只读扫描、解析并写入索引；后续生成执行增量同步。同步时枚举 canonical 文件并比较 `path + fingerprint`，只重新读取新增或变化文件，删除已消失文件的派生事实，并以单调 `index_revision` 标识索引版本。不使用目录 mtime 作为唯一变化依据；索引损坏或版本不匹配时允许整体重建。

物理模型压缩为 8 张表，逻辑领域通过视图保留：

| 物理表 | 领域事实 |
| --- | --- |
| `analytics_sources` | 源文件路径、类型、fingerprint、解析状态和错误码 |
| `analytics_index_state` | 单例 watermark、index revision 和同步状态 |
| `analytics_tasks` | 任务 locator、标题、模式、状态、活动时间，以及冗余的 `projectLocator`、`projectName` |
| `analytics_runs` | 统一执行单元 locator、任务 locator、`unitType`、状态、outcome 和活动时间；`unitType` 覆盖 Workflow run、AUTO outer run 和 Direct reply |
| `analytics_attempts` | attempt locator、run locator、节点、Agent、`sessionElapsedSeconds`、补零标记，以及 input/output/cache read/cache write/total Token 汇总 |
| `analytics_counters` | 工具、权限、elicitation、暂停、恢复、手动继续、Skill 等有界计数；`ownerType`、`kind` 使用 CHECK 约束 |
| `analytics_semantic_samples` | 供 Agent 洞察使用的有界语义样本 |
| `analytics_insight_runs` | 一次洞察执行的 range、schemaVersion、indexRevision、agentType、状态、错误码和校验通过的 `insightsJson` |

`analytics_projects`、`analytics_usage`、`analytics_event_counts` 和 `analytics_insights` 不再是物理表，分别作为视图提供：

- `analytics_projects` 从 `analytics_tasks` 按 `projectLocator` 聚合项目数、任务数和活动时间。
- `analytics_usage` 从 `analytics_attempts` 聚合 Token。
- `analytics_event_counts` 从 `analytics_counters` 按 `kind + name` 聚合。
- `analytics_insights` 使用 SQLite JSON 函数展开 `analytics_insight_runs.insightsJson`。

这种压缩不改变事实边界：项目是任务上的展示投影，Token 的统计粒度仍是 attempt，工具和事件计数有显式 `kind` 白名单，洞察结果必须在完整校验通过后作为一个 JSON payload 原子写入。不得为了继续减表把 `tasks/runs/attempts` 合并成宽表，否则会引入重复行和统计放大；也不得使用无约束 EAV。

日期筛选使用本地时区自然日，起止日均包含在内。Run 按最后活动时间或终局时间归属；Direct 回复按回复活动时间归属；Token 和累计执行耗时跟随所属 attempt/run 归属。跨日期长任务的不同 run 可分别进入不同日期范围；单个 attempt 不按秒拆分，整体跟随最后活动时间，这是明确口径而非估算。

页面提供全部、今天、近 7 天、近 30 天和自定义范围。自定义范围使用两个原生日期输入，不新增日期库依赖。切换范围后从 SQLite 查询聚合，不重新解析 canonical 历史。

### 9.3 确定性报告与 Agent 洞察解耦

“开始分析”只执行索引同步和确定性报告生成，不调用 Agent。Agent 选择继续保留在页面顶部，但只服务于独立的“生成 Agent 洞察”按钮。

Agent 洞察基于当前日期范围和当前 `index_revision` 的投影生成，仍只输出质量、效率、Token、上下文与技能四类结构化洞察。洞察插入对应章节的“Agent 洞察”子区块，不集中放在报告末尾。Schema 不合法时仍最多执行一次结构 repair。

日期范围或 `index_revision` 变化后旧洞察不可复用；相同范围和索引版本下已完成的洞察可复用，避免重复调用 AI。洞察失败、取消或校验失败不影响确定性统计报告。

### 9.4 设计评估

- 根因评估：当前痛点来自数据生命周期设计不足。日期筛选要求同一历史数据可反复按不同范围查询，用户增量使用要求避免每次全量重读；AI 与确定性报告耦合又让统计展示依赖 Agent 成功。SQLite 派生索引和洞察独立生命周期是根本修复。
- 现成能力评估：复用现有 SQLite/rusqlite、WAL、事务、迁移机制、原生日期输入、shadcn/ui 和 IntersectionObserver。行业上以文件为事实源、数据库为可重建投影属于成熟实践；无需引入第三方分析数据库。
- 过度设计评估：新增分析索引是必要的，但物理表压缩为 8 张并用视图保留逻辑领域，不创建第二个数据库、不保存无限报告历史、不建设通用缓存平台。语义样本、洞察结果和排行仍必须有界。当前设计没有引入假设性的队列、并发框架或服务端同步。
- 性能评估：首次全量解析仍为 O(eligible canonical bytes)；当前 release 约 `19.330s`，略慢于同轮既有投影路径 `17.600s`，且未达到历史 `6.032s` 门槛。增量同步只读取新增或变化文件，实测约 `9.758s`；日期切换只执行索引化 SQLite 聚合，实测约 `64ms`；页面导航不触发全报告重渲染。
- 数据完整性评估：文件是权威源，SQLite 可重建；写入采用事务和幂等 upsert，删除源文件时同步删除派生事实。项目、usage、事件计数和洞察明细视图没有独立事实版本，必须从 8 张物理表重建。洞察缓存仅对 completed payload 按 range/schema/indexRevision/agentType 建唯一索引，processing/failed/cancelled 是可重试生命周期事实，不阻塞后续成功。报告必须携带 `index_revision`，防止旧洞察或旧报告与新索引混合。
- 主要风险与缓解：fingerprint 计算增加元数据读取，但远低于重复解析 JSONL；日期归属存在跨天任务边界，必须显式使用最后活动时间口径；索引迁移失败需保留旧报告并支持重建；洞察缓存必须绑定 range、schema 和 index revision。

### 9.5 优化验收门槛

- 首次全量索引性能未达到 2026-08-17 `6.032s` 门槛：当前约 `19.330s`，也比同轮既有投影路径 `17.600s` 慢约 `1.730s`；该差距保持未关闭状态，禁止标记为达标。增量同步只读取变化文件，并用测试固定。
- 任意两个日期范围的 run、task、Token、累计执行耗时和覆盖率统计均可从索引复算并与接口结果一致。
- `analytics_projects`、`analytics_usage`、`analytics_event_counts` 和 `analytics_insights` 视图与物理表聚合结果一致，视图不持有第二份事实。
- 修改、新增、删除、损坏和未知版本文件均不会造成半写入索引或全量失败。
- 生成确定性报告不触发任何 AI 调用；Agent 洞察按章节展示，失败不影响统计。
- 页面导航、日期筛选和洞察按钮在 1280px、720px、浅色/深色主题下无重叠和横向溢出。
