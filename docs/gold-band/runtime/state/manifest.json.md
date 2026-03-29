# `manifest.json` 规范

## 1. 一句话定义
`manifest.json` 是 attempt 级产物清单，由 runtime 管理。

它用于表达：
- 本次 attempt 有哪些 canonical artifacts
- 每个 canonical artifact 的真实路径、来源和状态是什么

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "artifacts": {
    "exec-plan": {
      "path": "artifacts/exec-plan.json",
      "state": "resolved",
      "source": "runtime_normalized",
      "contentType": "json"
    }
  }
}
```

---

## 3. 必填字段
- `version`
- `artifacts`

---

## 4. 字段说明

### `artifacts`
- 类型：object
- key：logical artifact name
- value：artifact entry

### artifact entry 最小字段
- `path`
- `state`
- `source`
- `contentType`

#### `path`
- 类型：string
- 含义：相对 attempt 目录的路径

#### `state`
- 类型：string
- 枚举建议：`resolved | missing | invalid | ambiguous`

#### `source`
- 类型：string
- 枚举建议：
  - `runtime_normalized`
  - `runtime_generated`
  - `worker_side_effect`
  - `provider_stream`

#### `contentType`
- 类型：string
- 示例：
  - `json`
  - `markdown`
  - `text`
  - `jsonl`

---

## 5. 重要边界

### 5.1 manifest 只登记 canonical artifacts
- `artifacts` 只记录 canonical artifact
- `attachments/` 属于 free-form side effects，不进入最小 manifest contract

### 5.2 manifest 不直接决定控制流
控制流判断应基于：
- artifact 内容本身
- runtime 归纳出的 outcome

manifest 的作用主要是：
- 产物索引
- 路径映射
- 审计与展示

---

## 6. runtime 校验规则
以下情况应视为 `invalid`：

- 缺少任一顶层必填字段
- `artifacts` 不是对象
- 任意 entry 缺少 `path | state | source | contentType`
- `state` 不在合法枚举内

---

## 7. 相关文档
- [Runtime 概览](../overview.md)
- [目录布局](../layout.md)
- [Worker Invocation Contract](../../provider/invocation.md)
- [node.json](node.json.md)

---

## 8. 一句话总结

> `manifest.json` 是 attempt 目录中 canonical artifacts 的目录表：它不负责登记 free-form attachments，只为 runtime、CLI 和插件提供稳定的 canonical 路径映射与审计入口。
