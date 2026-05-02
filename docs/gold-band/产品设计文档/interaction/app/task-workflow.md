# 任务编排：任务工作流页

## 1. 一句话定义
任务工作流页用于展示单个 task 的原始 workflow 全貌，以及该 task 下按 run -> round 展开的执行历史。

---

## 2. 页面入口
进入方式：
- 从任务列表点击某个任务
- 从任务详情点击“工作流”
- 从 round 详情面包屑返回“工作流”

页面面包屑：

```text
任务列表 > 任务01 > 工作流
```

---

## 3. 页面结构

```text
┌──────────────────────────────────────────────────────────────┐
│ 面包屑：任务列表 > 任务01 > 工作流                            │
│ 任务标题 / requirement 摘要 / 当前状态                         │
├──────────────────────────────────────────────────────────────┤
│ 原始 workflow 全貌图                                           │
│ prepare -> plan -> execute -> validate -> finalize             │
├──────────────────────────────────────────────────────────────┤
│ run / round 执行列表                                           │
│ run-001                                                       │
│   round-001   success / artifacts / duration                   │
│   round-002   failure / validation failed                      │
│ run-002                                                       │
│   round-001   running / current node                           │
└──────────────────────────────────────────────────────────────┘
```

---

## 4. 顶部任务摘要
展示当前 task 的稳定上下文：
- task id
- title
- requirement 摘要
- workflow 校验状态
- 当前 active run
- 最近 outcome
- artifact 总数

操作：
- 新建 run
- 继续运行
- 停止当前 run
- 查看 requirement

危险操作如停止当前 run 必须使用明确的危险色和确认提示。

---

## 5. 原始 workflow 全貌图

### 5.1 定义
顶部 workflow 图展示 task authoring 阶段解析出的原始 workflow。

它表达的是：
- workflow 的设计结构
- 节点顺序
- 条件路径
- success / failure / invalid 分支

它不表达某一次 round 的实际执行细节。

### 5.2 节点展示
每个节点展示：
- node id
- node type
- 简短 label
- 是否有历史 artifacts
- 最近执行 outcome 摘要

### 5.3 交互
- 单击节点：在页面内显示该节点的跨 run 摘要。
- 双击节点：过滤下方 run / round 列表，仅看涉及该节点的 round。
- 右键节点：显示节点级操作菜单，如复制 node id、查看历史 attempts。

---

## 6. Run / Round 执行列表

### 6.1 排列方式
下方列表按 run 分组，run 内按 round 展开：

```text
run-001
  round-001
  round-002
run-002
  round-001
```

默认排序：
- 最新 run 在上
- run 内 round 按执行顺序展示

### 6.2 Run 行
Run 行展示：
- run id
- status
- outcome
- startedAt / finishedAt
- round 数量
- 当前 round
- resumable 状态

Run 行操作：
- 展开 / 收起
- Resume
- Stop
- 查看 run artifacts

### 6.3 Round 行
Round 行展示：
- round id
- index
- status
- outcome
- trigger
- repairLoopsUsed
- 当前节点或失败节点
- artifact / attachment 数量
- duration

点击 round 行进入 round 详情页。

页面层级变为：

```text
任务列表 > 任务01 > 工作流 > run01 > round01
```

---

## 7. 运行状态表达
工作流页需要同时展示两层状态：

### 7.1 Workflow 设计状态
来自原始 workflow 解析结果：
- valid
- invalid
- missing

### 7.2 Run / Round 执行状态
来自 canonical state：
- running
- paused
- completed
- success
- failure
- killed

不应根据 raw stream 或日志直接推断终局状态。

---

## 8. 一句话总结

> 任务工作流页上半区看“原始 workflow 设计”，下半区看“这个 workflow 在每次 run / round 中实际跑成了什么样”。
