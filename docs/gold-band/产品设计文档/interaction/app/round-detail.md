# 任务编排：Round 详情页

## 1. 一句话定义
Round 详情页用于查看某个 run 中某一轮 round 的实际执行图、全局信息流，以及日志、会话、artifact、attachment 的详细内容。

---

## 2. 页面入口
进入方式：
- 在任务工作流页点击某个 round 行
- 从 run 摘要中进入当前 round
- 从失败、可恢复、正在运行状态直接进入对应 round

页面面包屑：

```text
任务列表 > 任务01 > 工作流 > run01 > round01
```

---

## 3. 页面结构

Round 详情页采用三块区域：

```text
┌──────────────────────────────┬──────────────────────────────┐
│ 左上：实际工作图              │ 右部：详细信息查看页            │
│ Actual Round Graph           │ Detail Viewer                 │
├──────────────────────────────┤                              │
│ 左下：全局信息流              │                              │
│ Global Information Stream    │                              │
└──────────────────────────────┴──────────────────────────────┘
```

推荐比例：
- 左侧约 60%-65%
- 右侧约 35%-40%
- 左上约占左侧高度 45%-55%
- 左下约占左侧高度 45%-55%

三个区域均可滚动；左右分栏和上下分栏可调整大小。

---

## 4. 左上：实际工作图

### 4.1 定义
实际工作图展示当前 round 中真实发生的节点执行路径。

它不同于任务工作流页顶部的原始 workflow 图：
- 原始 workflow 图表达设计全貌。
- 实际工作图表达本 round 实际执行过、正在执行或等待执行的路径。

### 4.2 节点展示
节点卡片展示：
- node id
- node type
- status
- outcome
- latest attempt
- artifact 数量
- attachment 数量
- 当前是否运行中

### 4.3 节点高亮规则
以下节点需要视觉强调：
- 当前运行节点
- 失败节点
- paused / blocked 节点
- 有 artifacts 的节点
- 有 attachments 的节点
- 当前选中节点

其中 artifacts / attachments 可使用独立徽标，例如：

```text
A3  表示 3 个 artifacts
P2  表示 2 个 attachments
```

### 4.4 节点交互
- 单击节点：选中节点，左下信息流追加节点相关 artifacts / attachments。
- 双击节点：右部详情查看页打开节点摘要。
- 右键节点：打开上下文菜单。

节点右键菜单建议：
- 查看节点详情
- 查看会话
- 复制 node id
- 从该节点重试

---

## 5. 左下：全局信息流

### 5.1 选中 round 时
如果当前选中对象是 round，左下展示：
- 当前 task 的 requirement 摘要
- 当前 round 的状态摘要
- run / round 日志信息
- progress events
- validation 摘要

内容顺序建议：

```text
Requirement
Round Summary
Validation
Events
Runtime Log
```

### 5.2 选中 node 时
如果当前选中对象是 node，左下在 round 信息基础上追加：
- node 摘要
- node attempts
- node artifacts
- node attachments
- node 相关日志过滤结果

内容顺序建议：

```text
Requirement
Round Summary
Selected Node
Artifacts
Attachments
Events filtered by node
```

### 5.3 信息流交互
- 点击日志项：右部详情查看页打开日志详情。
- 点击 event：右部详情查看页打开 event JSON / 格式化说明。
- 点击 artifact：右部详情查看页打开 artifact 内容。
- 点击 attachment：右部详情查看页打开 attachment 内容。

---

## 6. 右部：详细信息查看页

### 6.1 定义
右部是详情查看区，不承担主导航。

它用于展示用户从左上实际工作图或左下信息流中选择的具体对象。

可展示对象包括：
- 日志详情
- event 详情
- node 摘要
- provider 会话引用
- artifact 内容
- attachment 内容
- validation 详情

### 6.2 默认状态
进入 round 详情页时，右部默认展示 round summary：
- round id
- run id
- status
- outcome
- trigger
- repairLoopsUsed
- startedAt
- 当前节点
- 最近错误摘要

### 6.3 查看日志
点击左下日志项后，右部展示：
- 日志时间
- 来源
- 级别
- 内容
- 关联 run / round / node / attempt

### 6.4 查看会话
右键节点选择“查看会话”后，右部展示：
- provider
- worker ref
- attempt id
- 会话状态
- 可打开原始 provider 会话的操作

Gold Band 默认只查看和 handoff，不在右部直接做聊天式接管。

### 6.5 查看 artifact / attachment
点击 artifact 或 attachment 后，右部展示：
- 名称
- 类型
- 来源 node
- 来源 attempt
- 更新时间
- validation 状态
- 内容预览

内容预览规则：
- JSON：格式化树或 pretty print
- Markdown：阅读视图
- 文本：plain text
- 图片：图片预览
- 不支持的二进制：展示 metadata 与打开文件位置

---

## 7. 返回与选择规则
- 点击面包屑返回上级页面。
- Esc 优先关闭右键菜单或浮层。
- 没有浮层时，Esc 从右部详情返回 round summary。
- 再次 Esc 可清空节点选择，回到 round 选中状态。
- 不通过命令输入返回。

---

## 8. 一句话总结

> Round 详情页左上看“这一轮实际怎么跑”，左下看“这一轮发生了什么”，右侧看“我点中的对象具体是什么”。
