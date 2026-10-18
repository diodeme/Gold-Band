# DeepSWE v1.1 hard 子集：Gold Band AUTO vs Claude Code

- 时间：2026-10-04 至 2026-10-05
- 结论：29 题 AUTO 通过 15，Claude Code 通过 12。方向有利但统计不显著（结果不同的 9 题中 AUTO 6 胜 3 负，双侧符号检验 p≈0.5）。AUTO 输出 token 约为 2.4 倍，耗时约 1.7 倍。
- 局限：题目在单个 Claude Code 会话 15–30 分钟内可完成，无法体现 Gold Band 在超长程任务上的价值；每题每边只跑 1 次。

## 1. 目的与选题

目的：验证 Gold Band AUTO 在长程编码任务上是否优于直接使用 Claude Code，用于早期宣传。

- Benchmark：[DeepSWE v1.1](https://github.com/datacurve-ai/deep-swe)（Datacurve，113 题，TypeScript/Python/Go/Rust/JavaScript，Harbor 任务格式，隐藏测试评分，Agent 运行时禁网）。
- 执行框架：[Pier](https://github.com/datacurve-ai/pier) 0.3.1（v1.1 要求的独立评分环境）。
- hard 子集：DeepSWE 官方博客公布的逐题 v1.1 通过率（每题 36 次运行平均）≤30% 的 29 题，列表见 [harness/hard-subset.txt](harness/hard-subset.txt)。选题在开跑前确定。
- 注意：官方通过率与排行榜均由极简框架 mini-swe-agent 跑出，"hard"反映的是该框架下的难度，不等于对 Claude Code 难。

## 2. 环境

| 项目 | 配置 |
| --- | --- |
| 机器 | 本机 Ryzen 9 5900X，WSL2 Ubuntu 24.04（装在 G 盘，24G 内存 / 20 核） |
| 容器 | WSL 内 Docker Engine 29；每题容器 2 CPU / 8G |
| Gold Band | 本仓库 `gold-band run` headless CLI，在 `rust:1-bookworm` 中构建 Linux 二进制（与题目镜像 Debian 12 同基线） |
| 网络 | Pier egress 代理，Agent 只能访问 `api.deepseek.com` |
| 超时 | 每题 Agent 3 小时（任务默认） |

环境注意事项：

- `.wslconfig` 需关闭 `vmIdleTimeout` / `instanceIdleTimeout`，否则无会话附着时 WSL 关机，后台任务被杀。
- 高并发时 Docker 默认网络地址池耗尽（`all predefined address pools have been fully subnetted`），需在 `/etc/docker/daemon.json` 设置 `default-address-pools`（本次用 `10.200.0.0/14`，`/24`）。
- 题目镜像来自 `public.ecr.aws`，匿名并发拉取会被限流（429），大批量前先串行预拉（[harness/prepull.sh](harness/prepull.sh)）。

## 3. 两组配置

| 项目 | AUTO 组 | Claude Code 对照组 |
| --- | --- | --- |
| 执行方式 | Pier 自定义 agent [gold_band_agent.py](harness/gold_band_agent.py) → `gold-band run` → claude-agent-acp 0.81.2 | Pier 内置 `claude-code` agent |
| Claude Code 版本 | 2.1.280（claude-agent-acp 内置） | 2.1.280 |
| 模型 | DeepSeek V4.1 Flash（`deepseek-flash[1M]`），所有别名与子 agent 同一模型 | 同左 |
| 思考强度 | `effort=max` | `--effort max` |
| 权限 | `bypassPermissions` + Auto Accept | `bypassPermissions` |
| 允许角色 | plan、dev、dev-test、test、review、accept | — |
| 动态预算 | 最多 40 节点、深度 10、fanout 5、并行 3、嵌套 fanout 2 层 | — |

已核实的差异（均属各自产品默认行为）：

- 后台任务：Gold Band 强制关闭；对照组开启（Pier 默认）。
- 思考：Gold Band 显式 adaptive + summarized 展示；对照组用默认值，日志确认每题有 84–259 个思考块，推理正常。
- 禁用工具：Gold Band 禁用 `Monitor`；对照组禁用 `EnterPlanMode`。
- Gold Band 内置提示词当时为中文（容器无语言设置时的默认值），题面为英文。

无人值守判定规则（适配器）：

- Gold Band 退出码 0/1 计为正常结果；3/4 或无退出码、或日志出现余额不足，标记 `GoldBandInfraError` 重跑。
- 退出码 2（暂停）且运行时错误全部为 `dynamic.*`（Gold Band 协议失败）时计为 Agent 失败，不重跑。
- 对照组未接入额度检测，额度耗尽需人工核对。

## 4. 结果

### 4.1 汇总

| 指标 | AUTO | Claude Code |
| --- | --- | --- |
| 通过 | **15/29（52%）** | 12/29（41%） |
| 平均耗时 | 约 37 分钟 | 约 22 分钟 |
| 未缓存输入 token | 11.6M | 3.7M |
| 缓存命中 token | 940M | 745M |
| 输出 token | 10.7M | 4.5M |

结果不同的 9 题：AUTO 独有通过 6 题（bandit、kombu、quill、effect、ink、katex），Claude Code 独有通过 3 题（termenv、oxvg、sqlfmt）。

### 4.2 逐题结果

F2P 为新功能隐藏测试通过数，P2P 为原有测试通过数（只列有回归的情况）。

| 题目 | 官方通过率 | AUTO | Claude Code |
| --- | --- | --- | --- |
| bandit-structured-nosec-directives | 0% | ✅ 69/69 | ❌ 69/69，P2P 281/282 |
| httpx-streaming-json-iteration | 28% | ✅ | ✅ |
| kombu-virtual-queue-dead-lettering | 22% | ✅ | ❌ 68/76 |
| koota-query-predicates | 22% | ✅ | ✅ |
| obsidian-linter-link-format-conversion | 28% | ✅ | ✅ |
| participle-grammar-conflict-analysis | 29% | ❌ 90/91 | ❌ 90/91 |
| python-statemachine-state-data-scoping | 25% | ✅（使用 fanout） | ✅ |
| quill-shared-toolbar-focus | 19% | ✅ | ❌ 11/13 |
| termenv-preserve-ansi-resets | 9% | ❌ 29/35 | ✅ |
| vulture-persistent-analysis-cache | 17% | ❌ 23/24 | ❌ 24/24，P2P 290/295 |
| clack-async-autocomplete-options | 22% | ✅ | ✅ |
| effect-sse-httpapi-streaming | 25% | ✅ | ❌ 44/47 |
| eicrud-keyset-pagination-cursor | 22% | ❌ 0/14 | ❌ 0/14 |
| expr-try-catch-errors | 19% | ❌ 78/79 | ❌ 78/79 |
| gql-incremental-graphql-delivery | 3% | ❌ 14/17 | ❌ 16/17 |
| happy-dom-deterministic-intersectionobserver | 11% | ❌ 13/14 | ❌ 13/14 |
| ink-grid-box-layout | 17% | ✅ | ❌ 21/25 |
| katex-multicolumn-array-spans | 19% | ✅ | ❌ 86/94 |
| koota-deferred-mutation-buffer | 22% | ✅ | ✅ |
| koota-pair-relation-tracking | 17% | ✅ | ✅ |
| langchain-request-coalescing | 25% | ❌ 49/50 | ❌ 49/50 |
| mashumaro-flattened-dataclass-fields | 25% | ✅ | ✅ |
| obsidian-linter-auto-table-of-contents | 0% | ❌ 0/41 | ❌ 0/41 |
| oxvg-structural-selector-preservation | 11% | ❌ 4/6 | ✅ |
| pest-character-class-coalescing | 14% | ❌ 98/104 | ❌ 103/104 |
| prometheus-transactional-reload-status | 11% | ✅ | ✅ |
| sqlfmt-create-table-ddl-formatting | 25% | ❌ 27/32，P2P 1257/1273 | ✅ |
| testem-bail-on-test-failure | 14% | ❌ 协议失败暂停，未评分 | ❌ 85/90 |
| updo-policy-alerting | 9% | ❌ 14/17 | ❌ 13/17 |

### 4.3 冒烟（旧配置，不计入对比）

允许全部角色、默认动态预算，随机 5 题：AUTO 通过 4（abs-module-cache-flags、goreleaser-retry-publish-auditing、psd-tools-blend-range-api、koota-pair-relation-tracking），drizzle-orm-window-function-builders 失败（126/130）。用 `nop` agent 对 3 道已通过题做空补丁对照，均为 0 分，确认评分器有效。

## 5. AUTO 失败分析

共同根因：首个节点（bootstrap / plan）对需求的解读成为后续所有节点的框架，review / test / accept 只核对"实现是否符合该解读"，没有独立回到原始需求重新推导，多节点因此产生相关性失误而非互相纠错。

| 题目 | 直接原因 |
| --- | --- |
| oxvg | plan 节点把 `remove_empty_containers` 列为"明确非目标"，并要求下游把非目标清单当红线；需求原文明确"被牵连元素可以是 selector 目标本身"，失败的 2 个隐藏测试正是该 job。7 节点、77 分钟没有任何节点质疑范围。 |
| termenv | 需求要求开启 PreserveResets 后在每次 reset 后重开外层样式；AUTO 只在截断路径实现，`Style.Styled()` 未实现（Claude Code 实现了）。review 做了 72,000 组随机测试，但全部针对截断函数。6 个失败测试中 5 个是 `Styled()` 场景。 |
| sqlfmt | 把强耦合的 DDL 模块与格式化拆成两个 fanout 分支；合并时改了原有词法规则，并同步修改原有测试 `test_rule.py` 去适配。accept 运行被修改过的测试全绿，评分恢复原测试后 16 个回归。`constrained_columns` 的返回类型也在分支内自行决定，合并后未对照需求。 |
| testem | 完成报告为单行、深度嵌套的 JSON（含多段 800–1400 字任务描述），只缺最外层的 `}`。3 次修复提示均带结构化错误（`dynamic.json.syntax`、`eof`、行列号、出错前文本、期望与建议），模型每次都重写整份 JSON（16193 → 12001 → 10648 → 9586 字符），内容各不相同，但每次都漏掉最后一个 `}`，运行以 `dynamic.completion.repair-exhausted` 暂停。反馈只说明"未闭合"，没有指出具体缺少哪个闭合符，且建议重写整份 JSON。 |

另见 Claude Code 组：bandit、vulture 在新功能全对的情况下引入原有测试回归。AUTO 只在 sqlfmt 出现一次回归，样本太少，不能据此宣传"更少回归"。

## 6. Gold Band 改进候选

以下为根因方向；1、2、4 已于 2026-10-05 实施（见开发计划同日条目），其余待定：

1. 【已实施】review / test / accept 先从原始需求逐条列出检查点，方案只能补充；非目标只能来自需求原文或用户，方案把需求原文内容排除在外即为 BLOCKER。
2. 【已实施】本轮改动过原有测试时，验收须用改动前的原版测试复验。
3. 强耦合交付不应 fanout；若拆分，合并后重新对照需求核对接口。主要取决于模型判断，暂不改动。
4. 【已实施】完成报告的总结与任务正文写入 attachments 文件，JSON 只用 `summaryPath` / `taskPath` 引用路径，runtime 校验后冻结正文。
5. 内置提示词语言跟随任务语言。

改进后冒烟（2026-10-05，`auto-handoff-smoke`，同配置，固定种子从未参与本次分析的题中抽 3 道非 hard 题：anko-typed-variable-bindings、arcane-drift-detection-baselines、opa-template-string-reconstruction）：

- 3 道均通过，退出码均为 0，总耗时 1 小时 21 分。
- 19 份完成报告全部使用路径字段、无内联正文，JSON 中位数 329 字符（改进前 hard 子集 4378），最大 426 字符。
- 校验错误 0、修复 0。
- 局限：只能证明协议改动可用。题目非 hard，样本量为 3，不能说明通过率变化；需求优先验收的效果需在新的 hard 题上复测。

## 7. 结论与下一步

- 本次数据只能说明：在单会话可完成的任务上，AUTO 不劣于 Claude Code，方向略优，代价是约 2.4 倍输出 token 和 1.7 倍耗时。
- 对外不宜引用"AUTO 比 Claude Code 高 11 个百分点"作为结论。
- 下一步优先评测超长程任务（[SWE-Marathon](https://arxiv.org/html/2606.07682v1)），以部分分为主指标；先选无 GPU、纯单元测试评分的题（如 zstd-decoder、wasm-simd、ruby-rust-port、rust-c-compiler），单题两边各跑一次实测成本后再扩量。
- Gold Band 改进后复测，需在未参与本次分析的新题上进行，原 29 题需全部重跑。

## 8. 复现

脚本均在 [harness/](harness/)，评测机目录为 WSL 内 `/root/gb-bench`：

1. [setup-base.sh](harness/setup-base.sh)：安装 Docker、Node 22、uv、Pier，克隆 DeepSWE。
2. [build.sh](harness/build.sh)：在 `rust:1-bookworm` 中构建 Linux `gold-band`。
3. [stage-credentials.sh](harness/stage-credentials.sh)：从本机 Claude Code 配置生成 `bench.env`（权限 600，不入库）。
4. [make-batches.sh](harness/make-batches.sh)：按固定种子打乱 hard 子集并分批。
5. 运行：`run-job.sh <job> -p <subset> -n 10`（AUTO）、`run-cc.sh <job> -p <subset> -n 10`（对照组），建议用 `systemd-run` 后台运行。
6. 重跑基础设施失败：`resume-job.sh <job> RuntimeError GoldBandInfraError`。
7. 观测与汇总：`watch.py <job> --loop 30`、`summarize.py <job-dir>`、`compare.py <auto-job> <cc-job>`。

原始结果（每题 Gold Band 运行目录、Claude Code 日志、补丁、评分输出）保存在评测机 `/root/gb-bench/jobs/`：`auto-hard-b1`、`cc-hard-b1`、`auto-hard-b23`、`cc-hard-b23`、`smoke-1`、`smoke-2`、`nop-control`。
