# `task.json` 规范

## 1. 一句话定义
`task.json` 保存任务级元数据，用于表达：

- 这个 task 是谁
- 当前处于什么阶段
- 当前激活的 requirement / workflow 是什么
- 最近一次 run 是什么

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "id": "task-20260320-001-login-error",
  "title": "Implement login error handling",
  "status": "active",
  "createdAt": "2026-03-20T10:00:00Z",
  "activeRequirement": "authoring/requirement.md",
  "activeWorkflow": "authoring/workflow.json",
  "latestRun": "runs/run-001"
}
```

---

## 3. 必填字段
- `version`
- `id`
- `title`
- `status`
- `createdAt`
- `activeRequirement`
- `activeWorkflow`
- `latestRun`

说明：
- `latestRun` 字段本身必须存在
- 但其值允许为 `null`
- 新建 task 且尚未启动任何 run 时，`latestRun = null`

---

## 4. 字段说明

### `version`
- 类型：string
- 首版固定为 `0.1`

### `id`
- 类型：string
- 含义：task 标识

### `title`
- 类型：string
- 含义：任务标题

### `status`
- 类型：string
- 枚举建议：`draft | ready | active | completed | archived`

说明：
- `draft`：需求与 workflow 仍在整理
- `ready`：可启动 run，但尚未开始
- `active`：当前仍在进行中
- `completed`：任务已结束
- `archived`：历史归档

### `createdAt`
- 类型：string
- ISO 8601 时间戳

### `activeRequirement`
- 类型：string
- 含义：当前激活 requirement 的相对路径
- 路径基准：task 目录

说明：
- 它代表 task 的稳定目标输入
- 验收失败进入新 round 时，不应因修复反馈而改写这里

### `activeWorkflow`
- 类型：string
- 含义：当前激活 workflow 的相对路径
- 路径基准：task 目录

说明：
- 它表达 task 当前默认 workflow
- 若 CLI 用 `--workflow` 做临时覆盖，不应直接修改这里

### `latestRun`
- 类型：string | null
- 含义：最近一次 run 的相对路径
- 路径基准：task 目录

说明：
- 字段必须存在
- 尚未有任何 run 时取 `null`

---

## 5. runtime 校验规则
以下情况应视为 `invalid`：

- 缺少任一必填字段
- `status` 不在合法枚举内
- `activeRequirement` 不是字符串
- `activeWorkflow` 不是字符串
- `latestRun` 既不是字符串也不是 null

---

## 6. 相关文档
- [Runtime 概览](../overview.md)
- [目录布局](../layout.md)
- [run.json](run.json.md)

---

## 7. 一句话总结

> `task.json` 是 task 级控制入口：它告诉 runtime 当前 task 的默认 requirement、默认 workflow，以及最近一次执行记录在哪里。
