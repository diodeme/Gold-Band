# 本次视觉重构的事实基线

核对日期：2026-10-14。这里只核对身份与资产；不重新运行 benchmark，也不根据网页搜索改写已归档的实验数据。

- Gold Band：官方仓库 https://github.com/diodeme/Gold-Band ，本地优先的桌面 Agent 工作流产品。存在 Direct、预定义 Workflow、AUTO；用户的仓库就是本次材料的权威来源。
- Claude Code：官方产品 https://claude.com/product/claude-code 。本报告中的名称指执行框架，不代表全部实验都使用 Claude 模型，也不代表禁止调用子 Agent。
- Kubernetes：以 CNCF 官方 artwork https://github.com/cncf/artwork/tree/main/projects/kubernetes 核对名称与标志；它在本次报告中是被重写的 benchmark 目标，不是受测模型或参赛执行框架。
- 已通过 WebSearch 核对上述产品身份；WebFetch 摘要服务遇到 429，未反复重试。标志下载通过 Node fetch 完成，SVG 内容检查通过。
- 不在设计稿中新增版本、发布时间、实时星标数或未经核实的产品规格。

## 两页样稿共用的记录结果

来源：`../README.md`、`../../2026-10-swe-marathon/README.md`。

| Kubernetes 同题对照 | 通过测试 | 记录 token | Agent 耗时 |
| --- | --- | --- | --- |
| Gold Band AUTO | 1,407 / 1,407 | 约 173M | 165 分钟 |
| Claude Code | 1,405 / 1,407 | 约 325.84M | 97 分钟 |

AUTO token 约少 47%，质量完整性略优，耗时仍更长。这是增加整体验收规则后的单次同题复测案例，不是所有长任务的平均收益。Ruff 未完成，不放入这两页的胜负数字。
