# 阶段结果与数据口径

材料截止 2026-10-13，台北时间。正文九页使用已归档结果；Ruff 的后续对照仍在运行，未把中途 visible 最好成绩当作最终 hidden 成绩。

## 执行框架，不是模型排名

Claude Code 在报告中指执行框架；其自身可调用子 Agent。DeepSWE、SWE-Marathon 使用 DeepSeek V4.1 Flash / max；mailauth 使用 DeepSeek V4.1 Flash / high；Ruff 使用 GPT-5.6-sol / high。不同 benchmark 不合并成一个总分，各自产品默认行为并非完全相同。

## DeepSWE 与 zstd

DeepSWE 为 29 道配对题，每臂每题一次：AUTO 15/29，Claude Code 12/29；输出 token 10.7M / 4.5M，平均耗时约 37 / 22 分钟。2.4× 指输出 token，不是总 token。配对符号检验 p≈0.5，不能推出稳定的普遍提升。

zstd 轻量工作流与 Claude Code 均 43/43；约 126M / 64M token，Agent 耗时 63 / 37 分钟。它是预定义工作流对照，不是 AUTO。

原记录：
- `docs/benchmark/2026-10-deepswe-hard/README.md`
- `docs/benchmark/2026-10-swe-marathon/README.md`

## mailauth

| 项 | 第一轮 AUTO | 第一轮 Claude Code | 第二轮 AUTO | 第二轮 Claude Code |
| --- | ---: | ---: | ---: | ---: |
| hidden | 694/702 | 684/702 | 701/702 | 702/702 |
| 总 token | 407,925,598 | 336,549,690 | 699,523,860 | 397,644,826 |
| Eval 耗时 | 3:54:37 | 2:42:06 | 8:09:05 | 2:29:52 |

四次运行 visible 均为 851/851。幻灯片总 token 取 M 单位一位小数，时间取整至分钟。总 token 包含缓存读取，不等于费用。

原记录：`docs/benchmark/2026-10-mirrorcode/README.md` 和该目录 `results/pair-01.json`、`pair-02.json`。

## Kubernetes 与 Stripe

Kubernetes 加入整体验收规则后的 AUTO 原始交付：1,407/1,407、约 173M token、165 分钟。Claude Code：1,405/1,407、约 325.84M token、97 分钟。记录 token 约少 46.9%，取约 47%；时间仍更长。

这是单次同题复测，不能当成全部长任务的平均收益。1,407 项中有 1,390 可见、17 隐藏；先前 AUTO 违规修改受保护文件后的人工追改成绩未纳入本页。两臂共享 WSL 内存，耗时不是完全隔离测量。

stripe-clone 已从本次演示移除，以下仅保留归档记录。该任务只有 AUTO 单臂：11/12 gate、pytest 82/83、约 52.1M token、66 分钟、reward=0。功能 gate 全过，源码扫描未过；无同版本 Claude Code 对照，不判断胜负。

原记录：`docs/benchmark/2026-10-swe-marathon/README.md`。

## Ruff

无闸门 Claude Code 已封存首段：visible 336/761，hidden 5/61（8.20%），309,442,360 总 token，8:02:18。它在仍能继续提升时主动结束；当时约 507K/1M 上下文、0 次压缩，提前退出不是由上下文触顶造成。

AUTO 已封存首段：visible 123/761，hidden 1/61（1.64%），637,609,395 总 token，13:56:15。约 920K 上下文时，声明窗口与实际上游窗口不匹配，错误转译又阻碍恢复，导致异常暂停；规划中的规则族 fanout 尚未执行。后续修复并续跑，因此首段不是最终完成成绩。

带闸门 CC 增加了提交闸门与官方评测工具，AUTO 没有相同设置。第 6 页的“无需额外闸门”专指没有额外的 benchmark 提交闸门，不表示没有任务预算、验收约定或故障恢复。AUTO 恢复后，基础节点补交产物，Runtime 接续后继节点并进入 fanout；持续执行由编排推进，而不是另加提交闸门阻止主会话退出。运行中还有配置调整、恢复和分段迁移，不声称从未中断或无需任何人工干预。后续对照仍在推进，不声称已完成最终胜负判断。

原记录：`docs/benchmark/2026-10-mirrorcode-ruff/README.md` 及 `results/cc-ungated-r1.json`、`auto-r1-paused.json`。

## 任务规模与选型参考

全景页的行数均为任务范围口径的物理行，不含任务级测试目录：

| 任务 | 口径 | 行数 |
| --- | --- | --- |
| DeepSWE | 29 题官方参考补丁逐题新增行 | 443–1,031，中位 652 |
| zstd-decoder | 参考实现（单文件 C，无嵌入式测试） | 2,373 |
| mailauth | 参考实现（Rust 48 文件，含嵌入式测试）；实际交付 Python 两轮 AUTO 8,492 / 8,978、CC 4,918 / 4,948 | 18,881 |
| Kubernetes | 参考实现（rusternetes commit 5e41501c…，10 个 crate src/，已扣除 41,293 行 `#[cfg(test)]`；含测试为 174,513） | 133,220 |
| Ruff | 任务口径；归档粗扫 348,164 行含 formatter / server / dev crate 与嵌入式测试，不可比，未采用 | 约 250,000 |

- DeepSWE 实际交付补丁含测试与文档：AUTO 中位 1,671（28 题，1 题暂停无补丁）、CC 中位 1,447。
- Kubernetes 交付容器按任务配置自动删除，无交付行数可统计；上表为参考实现规模。
- 「5 万行」取自实测两侧任务的几何中点：mailauth（参考 18,881 行，Direct 更优）与 Kubernetes（参考 133,220 行，编排占优）之间约为 50,150 行。
- 行数与压缩次数是启发式参考建议，不是实测的精确阈值。压缩信号来自 Ruff：无闸门 CC 0 次压缩即提前退出；带闸门 CC 约 912K 上下文触发压缩后继续推进。

## 架构分析

第 7 页的上下文与漂移表述属于长任务的架构判断，不是 Ruff 已量化证实的因果结论。goal 约束续跑与完成条件，不消除单会话的容量边界；AUTO 同样需要防范局部会话的上下文问题。

AUTO 中模型提出任务拆分、single/fanout 与后继路由；Runtime 校验并维护运行图、状态和调度。验收 Agent 判断业务质量；顶层结束前验收是路由约定，并非程序必然保证业务正确。

原设计：`docs/gold-band/产品设计文档/dsl/nodes/ai-dynamic.md`、`analytics/value-analysis-report.md`。

仓库：[diodeme/Gold-Band](https://github.com/diodeme/Gold-Band)。以上结果以本次本地归档快照为准，不随线上内容自动变化。
