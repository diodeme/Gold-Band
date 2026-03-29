# Claude Code Provider 实现

## 1. 定位
`claude-code` 是 Gold Band 当前默认的 provider 实现。

它负责把 Gold Band 的 provider adapter 抽象，落到 Claude Code 这套具体工具能力上。

## 2. 当前角色
在当前文档体系里，Claude Code 承担两种角色：

1. **默认 authoring / deep-dive 工具**
   - 用于需求澄清
   - 用于 requirement / workflow DSL 生成
   - 用于按需深查原始会话

2. **默认 provider 实现**
   - 用于执行 `worker` 节点
   - 提供 worker reference
   - 在支持时提供原始流式输出

## 3. 当前已知映射

### 3.1 provider id
- `claude-code`

### 3.2 worker reference
当前典型继续引用可表现为：
- `session_id`

### 3.3 打开/继续会话
当前典型命令模板可表现为：

```bash
claude -c <session_id>
```

### 3.4 流式输出
Claude Code 当前可作为支持 raw stream 的 provider 之一。

## 4. 当前仍待细化
- Claude Code provider 的 `doctor()` 具体检查项
- `runWorker()` 的 Claude Code 参数映射
- Claude Code 原始 stream 到 Gold Band progress 的映射
- Claude Code provider 的 capability matrix

## 5. 相关文档
- [Provider 概览](../overview.md)
- [Provider Adapter 接口](../adapter.md)
- [Worker Ref 规范](../worker-ref.md)
- [Progress 规范](../../interaction/progress.md)
