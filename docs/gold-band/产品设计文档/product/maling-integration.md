# Maling 通用能力迁入（2026-11-17）

本次将 Maling fork 的通用能力迁入 Gold Band，保持默认桌面渠道的既有产品身份与流程。具体来源、边界和本地验收见[迁入验收](../../开发计划/Maling通用改动迁入验收-2026-11-17.md)。

体验涵盖首次启动引导、更多 Agent、文件引用与产物导航、草稿和排队完善、Workflow 布局与节点重试、缩放、侧栏分页、轮次导航、输入高度及主题壁纸。设计真源分别落在 interaction/app 的 first-run-onboarding、agent-management、conversational-runtime、context-management、settings、task-workflow 以及 runtime/control 和 runtime/state/run.json 文档。

Multica/WB 的资产分发、卸载、工单响应、新 Runtime 协议及心跳频率调整不在本次范围；既有 Multica 功能保持原契约。

数据与生命周期边界：Workflow 当前节点重试沿用同 Run 的 canonical attempt 转移，已结束失败节点重跑创建 successor Run 并封存来源；草稿／排队引用继续由原有持久实体管理；首次引导允许完成检测后稍后设置；会话导航是可丢弃、可重建的有界索引投影，不拥有新的业务状态。

过度设计评审：复用已有运行状态、配置提交与组件，不加入渠道适配层或第二事实源。性能评审：分页、诊断并发及文件大小边界沿用各领域约束；轮次导航进一步固定为 64 条分页／256 条驻留，避免历史规模影响交互关键路径。
