# `exec-plan` 规范

## 1. 一句话定义
`exec-plan.json` 表达由 `worker` 节点交给 `exec` 节点执行的结构化命令计划。

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "commands": [
    {
      "id": "build",
      "run": "pnpm build",
      "purpose": "验证项目构建通过",
      "cwd": ".",
      "timeoutSec": 600
    },
    {
      "id": "test-targeted",
      "run": "pnpm vitest tests/auth.test.ts",
      "purpose": "验证本次修改的核心测试通过"
    }
  ]
}
```

---

## 3. 必填字段
- `version`
- `commands`

每条命令必填：
- `id`
- `run`
- `purpose`

---

## 4. 可选字段
每条命令可选：
- `cwd`
- `timeoutSec`

说明：
- `cwd` 未声明时，默认使用 workspace root
- `timeoutSec` 表示该命令的超时上限，超时后应在执行结果中明确体现

---

## 5. 语义约束
- 在 `worker` 调用语义下，`exec-plan` 通常应作为该次调用的 `primaryArtifact`
- `commands` 按数组顺序串行执行
- 首版不允许并行执行
- 首版不支持 group 调度
- 首版不支持依赖调度
- 首版不支持条件命令
- 不做 shell / 平台差异标准化，模型给什么命令，执行层就按该内容执行

---

## 6. runtime 校验规则
以下任一情况都应视为 `invalid`：

- `commands` 不是数组
- `commands` 为空数组
- 任意命令缺少 `id | run | purpose`
- 任意两个命令的 `id` 重复
- `cwd` 存在但不是字符串
- `timeoutSec` 存在但不是正整数

---

## 7. 相关文档
- [exec 节点](../nodes/exec.md)
- [worker 节点](../nodes/worker.md)

---

## 8. 一句话总结

> `exec-plan.json` 是 `exec` 节点唯一应程序化消费的命令计划输入；它描述“按什么顺序跑什么命令”，但不描述“跑成什么结果”。