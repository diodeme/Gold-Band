# `exec-plan` 规范

## 1. 一句话定义
`exec-plan.json` 表达由 `worker` 节点交给 `exec` 节点执行的结构化命令计划。

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "groups": [
    {
      "id": "build",
      "onFailure": "fail-fast",
      "commands": [
        {
          "id": "build",
          "run": "pnpm build",
          "purpose": "验证项目构建通过"
        }
      ]
    },
    {
      "id": "test",
      "dependsOn": ["build"],
      "onFailure": "fail-fast",
      "commands": [
        {
          "id": "test-targeted",
          "run": "pnpm vitest tests/auth.test.ts",
          "purpose": "验证本次修改的核心测试通过"
        }
      ]
    }
  ]
}
```

---

## 3. 必填字段
- `version`
- `groups`

每个 group 必填：
- `id`
- `onFailure`
- `commands`

每条命令必填：
- `id`
- `run`
- `purpose`

---

## 4. 可选字段
每个 group 可选：
- `dependsOn`

每条命令可选：
- `cwd`
- `timeoutSec`

示意：

```json
{
  "id": "build",
  "onFailure": "fail-fast",
  "commands": [
    {
      "id": "build",
      "run": "pnpm --filter web build",
      "purpose": "验证 web 包构建通过",
      "cwd": ".",
      "timeoutSec": 600
    }
  ]
}
```

---

## 5. 语义约束
- 在 `worker` 调用语义下，`exec-plan` 通常应作为该次调用的 `primaryArtifact`
- group 内命令按数组顺序执行
- `onFailure = fail-fast` 时，当前 group 在命令失败后立即停止后续命令
- 依赖失败的 group 应标记为 `skipped`
- 不依赖失败 group 的其他 group 仍可继续执行
- 首版不支持条件命令

---

## 6. runtime 校验规则
以下任一情况都应视为 `invalid`：

- `groups` 不是数组
- `groups` 为空数组
- 任意 group 缺少 `id | onFailure | commands`
- 任意两个 group 的 `id` 重复
- `onFailure` 不属于 `fail-fast | continue`
- `dependsOn` 存在但不是字符串数组
- `dependsOn` 引用了不存在的 group
- group 依赖关系存在环
- 任意命令缺少 `id | run | purpose`
- 同一 group 内任意两个命令的 `id` 重复
- `timeoutSec` 存在但不是正整数

---

## 7. 相关文档
- [exec 节点](../nodes/exec.md)
- [worker 节点](../nodes/worker.md)

---

## 8. 一句话总结

> `exec-plan.json` 是 `exec` 节点唯一应程序化消费的命令计划输入；它描述“跑什么”，但不描述“跑成什么”。
