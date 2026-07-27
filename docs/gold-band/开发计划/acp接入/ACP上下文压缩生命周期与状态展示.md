# ACP 上下文压缩生命周期与状态展示

## 目标

解决长会话 compact 期间只有普通 `Compacting...` 文本、用户无法判断是否仍在运行，以及异常结束后状态可能长期不收敛的问题。

## 实施范围

1. 在 ACP session update 归一化边界精确识别 compact 开始与完成控制消息。
2. 建立稳定的 `contextCompaction` timeline 生命周期，支持 running、completed、interrupted。
3. 记录并展示压缩前上下文占用。provider 的 `used=0` reset 与后续正数 usage 仍可作为诊断数据补写压缩后占用，但 Claude ACP adapter 当前混用完整上下文与 message-token proxy，压缩后值暂不进入 UI；不以该值判断压缩收益。
4. 消息流增加轻量 compact 状态行；composer 增加 `compacting` processing kind。
5. prompt 在 active compact 期间结束、取消或失败时，将 compact 收敛为 interrupted。
6. 增加结构化诊断、Rust 单元测试、前端 timeline/composer 回归测试和桌面 UI 验证。

## 既有能力复用

- timeline 继续复用现有稳定 item/patch 与 live-update 通道，不新增 Tauri IPC。
- prompt 提交继续复用唯一 `promptId`、terminal lifecycle 覆盖 optimistic 状态、停止/失败清理机制；不重复建立第二套发送状态机。
- UI 继续复用 prompt-kit 消息布局、Tailwind 语义 token 和现有 composer 停止入口。

## 验收标准

- `Compacting...` 不再显示为普通 assistant 气泡。
- 开始与完成只显示一个稳定 compact 条目。
- running 状态每秒更新已耗时，显示不定进度，不显示虚假百分比。
- completed 状态显示总耗时，并只显示压缩前 token 与上下文窗口上限；不显示箭头或压缩后 token。
- active compact 遇到 prompt terminal 时显示 interrupted，重新打开任务不会恢复成永久 running。
- composer 在 compact 期间显示“正在压缩上下文”，完成后自动回到后续 processing/responding 阶段。
- 普通包含 Compacting 字样的 assistant 文本不被误识别。
- 中英文文案、深色主题、减少动画偏好和 screen reader 状态播报均可用。

## 明确不包含

- 外部会话同步开启后，provider compact summary 被归类为 External user prompt 的边界问题，本期不处理。
- ACP provider 未提供的百分比、子阶段或服务端心跳，不通过客户端猜测补造。
