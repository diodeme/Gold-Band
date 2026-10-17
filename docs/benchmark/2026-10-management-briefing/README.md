# Benchmark 管理层阶段性汇报

**当前选定：B 编辑式。** [打开完整 HTML](editorial/index.html#1) · [PDF 备份](editorial/Gold-Band-Benchmark.pdf) · [使用说明](editorial/README.md)

[原版 HTML 留档](index.html)。以下保留最初汇报稿的使用说明和完整数据依据。

- 汇报日期：2026-10-14。
- 材料截止：2026-10-13，台北时间（UTC+8）。Ruff 仍在运行，不自动更新本文件。
- 听众：了解 AI 架构、不需要底层实现细节的管理者。
- 形式：9 页离线 HTML PPT。主页面讲结论，题目内容和证据点击展开。

## 使用

双击 `index.html`，用 Edge / Chrome 打开即可；不需要服务器、网络或安装依赖。单独复制 HTML 也能演示；相邻 README 与仓库来源链接需要保留目录才能打开。

- `←` / `→`、`PageUp` / `PageDown`：翻页。
- `Space`：下一页；`Home` / `End`：首页 / 末页。
- `F` 或“全屏”按钮：全屏。
- “测试全景”“使用建议”：快速跳转。
- 测试全景中点击 benchmark，展开具体内容；再次点击收起。保留原生键盘与焦点操作。
- 支持浏览器前进/后退，以及 `index.html#marathon` 等 deep link。
- 浏览器打印支持横向输出；打印时展开详情，结束后恢复演示状态。

## 汇报顺序与讲述重点

| 页 | 内容 | 建议讲述 |
| --- | --- | --- |
| 1 | 测试意图 | 我们要找到协作的价值边界：质量、token、时间，三者一起看；Claude Code 允许分发 subagent。 |
| 2 | 结论与任务全景 | 先给出“中短任务用 Direct，长任务用编排”，再用从轻到重的五种任务支撑；每行附参考实现行数。参考建议：新增/修改超过 5 万行或上下文压缩 3 次及以上，优先工作流 / AUTO。 |
| 3 | DeepSWE + zstd | AUTO 解题略多，工作流在 zstd 同分；额外协调尚未换来足够收益。 |
| 4 | mailauth | 与 Claude Code 质量接近，AUTO 的 token 与耗时更高。 |
| 5 | Kubernetes | 完整验收是长任务的重要价值；这次 AUTO 全过，记录 token 约少 47%，仍更慢。 |
| 6 | Ruff 阶段观察 | Claude Code 未完成就主动退出；AUTO 通过编排持续推进，无需额外提交闸门。故障与恢复放在展开记录；最终验收未完成。 |
| 7 | 上下文与漂移 | 按长任务架构分析讲解：goal 推动续跑，但不能解决主编排会话的容量边界。 |
| 8 | AUTO 架构 | 模型拆任务，程序管理状态与调度；fanout 分担上下文，合并后统一验收。 |
| 9 | 选型 | 短题 Direct；固定流程用工作流；目标明确但路径动态的长题用 AUTO。 |

## 数值与原始依据

**Claude Code 是执行框架名称，不是这里的底层模型。** DeepSWE 和 SWE-Marathon 使用 DeepSeek V4.1 Flash / max；mailauth 使用 DeepSeek V4.1 Flash / high；Ruff 使用 GPT-5.6-sol / high。Claude Code 对照不人为禁用自身子 Agent，因此“单体”指单主会话驱动，而不是绝对只有一次模型调用或完全没有子 Agent。

### 1. DeepSWE v1.1 hard

来源：[2026-10-deepswe-hard/README.md](../2026-10-deepswe-hard/README.md)，第 3–5、34–49、61–69、75–105 行。

| 项 | AUTO | Claude Code |
| --- | ---: | ---: |
| 通过题数 | 15/29 | 12/29 |
| 平均耗时 | 约 37 分钟 | 约 22 分钟 |
| 输出 token | 10.7M | 4.5M |
| 未缓存输入 token | 11.6M | 3.7M |
| 缓存读取 token | 940M | 745M |

- 演示中的 2.4× 是 **输出 token** 比，不是总 token 比。
- 若按报告舍入分项相加，总 token 约 962.3M / 753.2M，即 1.28×。
- 29 道同题配对，每臂每题一次；不同结果的 9 题中 AUTO 6 胜、Claude Code 3 胜，双侧符号检验 p≈0.5。
- 任务例子包括流式 JSON、死信队列、编辑器工具栏焦点、分页和 SQL 格式等。

### 2. SWE-Marathon / zstd-decoder

来源：[2026-10-swe-marathon/README.md](../2026-10-swe-marathon/README.md)，第 15–16、25 行。

| 项 | 轻量工作流 | Claude Code |
| --- | ---: | ---: |
| 通过 | 43/43 | 43/43 |
| token | 约 126M | 约 64M |
| Agent 耗时 | 63 分钟 | 37 分钟 |

两边同分。token 约 1.97×，时间约 1.70×。这是实际的预定义工作流对照，不能把它标为 AUTO。

### 3. MirrorCode / mailauth

来源：[2026-10-mirrorcode/README.md](../2026-10-mirrorcode/README.md)，第 9–34、48–69、112–136 行；精确记录：[pair-01.json](../2026-10-mirrorcode/results/pair-01.json)、[pair-02.json](../2026-10-mirrorcode/results/pair-02.json)。

| 项 | 第一轮 AUTO | 第一轮 CC | 第二轮 AUTO | 第二轮 CC |
| --- | ---: | ---: | ---: | ---: |
| 隐藏测试 | 694/702 | 684/702 | 701/702 | 702/702 |
| 总 token | 407,925,598 | 336,549,690 | 699,523,860 | 397,644,826 |
| Eval 耗时 | 3:54:37 | 2:42:06 | 8:09:05 | 2:29:52 |

- 第一轮在 10 月 7 日完成；第二轮从 10 月 7 日晚开始，10 月 8 日完成。
- 所有运行可见测试均为 851/851。总 token 含缓存读取。
- token 比约 1.21×、1.76×；时间比约 1.45×、3.26×。
- 演示的分钟按最近整分钟取整，精确值保存在展开说明中。
- 第三轮未完成，不存在第三轮成绩。

### 4. SWE-Marathon / stripe-clone

来源：[2026-10-swe-marathon/README.md](../2026-10-swe-marathon/README.md)，第 21、29、33–34 行。

AUTO：11/12 gate、pytest 82/83、约 52.1M token、66 分钟，最终 reward=0。11 个功能 gate 通过，anti_cheat 源码扫描未通过。

该任务展示了“验收发现 BLOCKER → 修复 → 再验收”的运行链，但没有同版本 Claude Code 对照。已按用户要求从当前 B 编辑版演示移除，此处仅保留归档依据；不作为优势结果，不引用旧题面榜单排名。

### 5. SWE-Marathon / kubernetes-rust-rewrite

来源：[2026-10-swe-marathon/README.md](../2026-10-swe-marathon/README.md)，第 17–19、26–28、33–36 行。

| 项 | AUTO（增加整体验收规则后） | Claude Code |
| --- | ---: | ---: |
| 通过 | 1,407/1,407 | 1,405/1,407 |
| reward | 1 | 0 |
| token | 约 173M | 约 325.84M |
| Agent 耗时 | 165 分钟 | 97 分钟 |

- Claude Code token 为约 324M input（其中约 321M cache）+ 1.84M output。
- AUTO 相对少 `(325.84 - 173) / 325.84 ≈ 46.9%`，演示取约 47%。token 不是金额。
- 任务是 Rust 重写 Kubernetes；AUTO 采用 foundation → 五类并行开发 → merge → all-in-one 集成 → final acceptance。
- 测试包括 1,390 项可见、17 项隐藏；差距集中在最后两项行为，不夸大为整体能力断层。
- 这是单次完成案例，且 AUTO 在分析先前失败、增加整体验收规则后同题复测，不是全新 holdout。
- 先前 AUTO 违规修改受保护 Cargo.toml，原始交付无法编译；人工追改后的 1,406/1,407 **不计作原始成绩，也未放入演示的成绩表**。
- 两臂并行运行，共享 WSL 内存，耗时非完全隔离基准。

### 6. MirrorCode / Ruff

来源：[2026-10-mirrorcode-ruff/README.md](../2026-10-mirrorcode-ruff/README.md)，第 9–14、22–43、47–71、75–109 行；封存记录：[cc-ungated-r1.json](../2026-10-mirrorcode-ruff/results/cc-ungated-r1.json)、[auto-r1-paused.json](../2026-10-mirrorcode-ruff/results/auto-r1-paused.json)。

| 已封存首段 | CC 无闸门 | AUTO 异常暂停 |
| --- | ---: | ---: |
| 可见测试 | 336/761 | 123/761 |
| 隐藏测试 | 5/61，8.20% | 1/61，1.64% |
| 总 token | 309,442,360 | 637,609,395 |
| Eval 耗时 | 8:02:18 | 13:56:15 |

- CC 无闸门运行在尚能继续提升时主动结束；当时约 507K/1M 上下文，0 次压缩，**提前退出并不是上下文触顶造成**。
- AUTO 首段约 920K 上下文，因声明窗口与上游实际窗口不一致及错误转译而暂停，规则族 fanout 尚未执行。后续修复并续跑，不能据首段认定 AUTO 已胜出，也不能说 AUTO 永不触碰窗口限制。
- 带闸门 CC 后续主会话到达约 912K，曾调整压缩窗口并续跑；此处支持“长程执行确实遇到容量边界”，但不证明某次质量损失由漂移造成。
- 后续两臂仍在分段运行。此次只读观察到 s14 running，不把阶段 visible best 当作 hidden 最终成绩。
- 带闸门 CC 增加了提交闸门和官方评测工具，而 AUTO 没有相同两项设置；运行中还发生恢复、配置调整与分段迁移。它们不是严格单变量对照。
- 为避免报告很快过期，主页面不展示运行中累计 token / 当前最好可见分，也不据此计算最终优势倍数。

## 架构分析的边界

演示第 7 页按汇报要求保留“持续使用 goal → 主编排会话触碰上下文限制 → 漂移”的表述，**归类为长任务架构判断，不是 Ruff 已量化证实的因果实验结论**。原始材料没有足够证据证明单体必然漂移，或漂移仅发生于单体；AUTO 自身也需要防范上下文与需求理解问题。

第 8 页的准确职责分工：模型负责理解任务、提出 single/fanout 与后继路由，Runtime 校验并维护运行图及调度。整体验收由验收 Agent 执行，顶层结束前验收是路由提示词约定，并非程序必然保证业务正确。

依据：

- [AI-DYNAMIC 设计](../../gold-band/产品设计文档/dsl/nodes/ai-dynamic.md)，第 40–56、60–66 行。
- [模式价值口径](../../gold-band/产品设计文档/analytics/value-analysis-report.md)，第 26–37、48–55 行。
- [Workflow DSL](../../gold-band/产品设计文档/dsl/overview.md)，第 4–17 行。

## 实现与验证

- 导航以项目 `ckm-slides` HTML 模板为起点，改为 hash 导航；正文为静态、可审阅快照，原生 `details` 用于展开。
- 只新增汇报文件，不修改产品功能、设计或 MVP 计划，不改写原始 benchmark，不启动、暂停或续跑实验。
- 没有额外依赖、API、遥测、定时轮询或持久缓存；9 页、6 个题目卡片，数据规模固定。
- 过度设计评审：不引入演示框架、构建流程或新的产品页面，现有模板与浏览器能力足够。
- 性能评审：单份有界静态 HTML；切页仅更新 9 个可见状态，不读取日志，不触发网络请求或扫描。
- 交互单元测试：`node --test docs/benchmark/2026-10-management-briefing/presentation.test.mjs`。
- 2026-10-14 验收：10/10 单元测试通过；真实 Chromium 中验证了 1280×720、1440×900、1920×1080，九页收起态均无横向或纵向溢出。
- 390×844 窄屏：九页无横向溢出，正文可滚动，导航保持可见；重新拉宽后布局正常。
- 实际操作通过：benchmark 展开、互斥展开、summary 空格键、方向键翻页、End、deep link、全屏进入与退出；页面错误列表为空。
- 打印展开与恢复状态通过接口测试；未另外生成 PDF。没有产品构建步骤，因为本交付仅为离线静态文档。
- 测试通过不代表正在运行的 benchmark 已完成。
