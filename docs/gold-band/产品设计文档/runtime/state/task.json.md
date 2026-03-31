# `task.json` 规范

## 1. 一句话定义
`task.json` 保存 task 级元数据，用于表达：

- 这个 task 是谁
- 这个 task 是做什么的
- 当前处于什么阶段
- 当前激活的 requirement / workflow 是什么
- 最近一次 run 是什么

---

## 2. 当前实现的最小结构

```json
{
  "version": "0.1",
  "id": "task-001",
  "title": "Task title",
  "description": "Short task description"
}
```

说明：
- 当前代码已稳定支持 `version / id / title / description`
- 其中 `title` 与 `description` 允许为 `null`
- Console Task Picker 当前依赖 `description` 作为主要展示摘要字段

---

## 3. 当前实现字段说明

### `version`
- 类型：string
- 首版固定为 `0.1`

### `id`
- 类型：string
- 含义：task 标识
- 当前实现要求非空

### `title`
- 类型：string | null
- 含义：任务标题

### `description`
- 类型：string | null
- 含义：任务说明摘要
- 当前主要用于 Welcome 后的 Task Picker 展示

---

## 4. 后续扩展方向
设计上仍可继续扩展到更完整的 task 管理字段，例如：
- `status`
- `createdAt`
- `activeRequirement`
- `activeWorkflow`
- `latestRun`

但这些字段当前不是 runtime 最小可运行集的一部分。

---

## 5. runtime 校验规则
当前实现下，以下情况应视为 invalid：
- `version != "0.1"`
- `id` 为空字符串

`title` 与 `description` 当前允许为空或缺失。

---

## 6. 与 Console 的关系
Console 当前会在 Task Picker 中读取并展示：
- `id`
- `title`
- `description`
- authoring/workflow 校验结果
- latest/resumable run 摘要

因此 `description` 已从“可选增强信息”上升为当前 console UX 的重要展示字段。

---

## 7. 一句话总结

> `task.json` 当前是 task 级最小元数据入口：至少要表达 task 的身份与简要说明，其中 `description` 已成为 console task picker 的核心展示字段。
