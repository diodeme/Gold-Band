# Gold Band Benchmark

记录 Gold Band 在公开 benchmark 上的评测：选题依据、执行环境、配置、结果、失败分析和后续计划。每次评测一个子目录，原始结果保留在评测机，文档只记录可复核的结论与复现方法。

## 记录原则

- 对照组与实验组同模型、同 Claude Code 版本、同思考强度、同机器与同超时；差异只能是 Gold Band 的编排。
- 选题标准必须在开跑前由公开数据确定，并写明来源。
- 基础设施失败（环境构建、额度耗尽）只重跑失败项，不计分；Agent 自身失败（包括 Gold Band 协议失败）计为失败，不重跑。
- 单次运行的结论只写"方向"，统计不显著时不得写成"显著优于"。
- 修复 Gold Band 后复测，必须在未参与分析的新题上验证，避免对已分析题目过拟合。

## 评测记录

| 日期 | 评测 | 结论摘要 |
| --- | --- | --- |
| 2026-10-04 | [DeepSWE v1.1 hard 子集：AUTO vs Claude Code](2026-10-deepswe-hard/README.md) | 29 题 AUTO 15 vs Claude Code 12，方向有利但不显著；题目规模在单会话能力内，不能体现超长程优势 |

## 候选 benchmark

| Benchmark | 适用性 | 备注 |
| --- | --- | --- |
| [DeepSWE v1.1](https://deepswe.datacurve.ai/blog/deepswe-v1-1) | 已评测 | 113 题，Harbor 格式，单题 15–60 分钟 |
| [SWE-Marathon](https://arxiv.org/html/2606.07682v1) | 下一步首选 | 20 个超长程任务（人类 40–400 小时），平均 27.2M token/次，最好配置通过率 <30%；Harbor 格式，有部分分 |
| [MirrorCode](https://arxiv.org/pdf/2606.30182) | 备选 | 25 个程序按行为重写，单题成本可达数千美元，只适合挑小题 |
