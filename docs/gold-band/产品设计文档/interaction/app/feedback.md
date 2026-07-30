# 用户反馈上报

## 1. 一句话定义

用户反馈上报是桌面端的渠道能力：用户主动提交问题描述，可选择截图、运行日志和一个关联会话，由后端在可信边界内解析、规范化并上传到码灵控制台。

当前 MVP 只实现主动反馈；崩溃自动收集不在本期范围。

---

## 2. 能力与入口

- 渠道配置通过 `feedbackEnabled` 显式声明能力：`wb=true`，默认渠道为 `false`。
- `AppInfoVm.feedbackEnabled` 是前端唯一入口判断，不根据渠道名称猜能力。
- 共享顶栏仅在能力启用时展示「帮助 → 用户反馈」。
- Tauri command 在后端再次校验能力；隐藏 UI 不是安全边界，未启用渠道直接返回 `feedback.disabled`。
- `feedbackEnabled` 是编译期渠道能力，不是用户偏好，不新增运行时开关或兼容入口。

---

## 3. Dialog 与交互

Dialog 复用 shadcn/ui `Dialog`、`Textarea`、`Select`、`Switch`、`Button` 和现有附件 copy-in 组件。

| 字段 | 必填 | 约束 |
| --- | --- | --- |
| 问题描述 | 是 | 1–2000 字符 |
| 关联会话 | 否 | 从已注册会话工作空间的任务列表单选 |
| 截图 | 否 | 0–4 张；PNG/JPEG/WebP；单张输入及规范化输出均 ≤5 MiB |
| 上传日志 | 否 | 默认开启；只上传运行日志尾部 512 KiB |

截图交互支持隐藏的原生 HTML file input、拖放和剪贴板粘贴。反馈流程不调用返回本地路径的 Tauri 通用文件选择器；提交时只序列化浏览器 `File` 的 `{name,mime,size,dataBase64}`。无法取得 `File` 内容的 path-only 附件不得提交。

选择关联会话后展示归档未压缩大小和文件数。超出客户端策略时显示错误状态并禁用提交；用户可取消关联会话后继续提交纯描述、截图与日志。

上传期间禁止重复提交和关闭。成功后展示简短成功状态并关闭；失败时保留 Dialog 和用户输入，前端按错误码展示本地化文案。

---

## 4. 数据与信任边界

### 4.1 输入模型

前端只传业务标识和内容，不传后端读取路径：

```text
FeedbackInput {
  description
  projectId? + taskId?
  screenshots[] { name, mime, size, dataBase64 }
  includeLogs
}
```

`projectId` 与 `taskId` 必须同时存在或同时为空。

### 4.2 会话解析

- 后端从全局 `StateConfig.conversationWorkspaces` 解析 `projectId`，再构造 workspace-scoped `App`。
- `taskId` 必须是单一安全路径组件，拒绝绝对路径、`.`、`..`、斜杠和反斜杠穿越。
- 任务目录及每个文件均 canonicalize，并验证仍位于 canonical `tasks_dir/taskId` 下。
- 目录遍历使用 `walkdir` 且 `follow_links(false)`；符号链接不进入归档。
- 未知项目、未知任务、已被移除的任务统一返回 `feedback.session-not-found`。

### 4.3 截图规范化

- base64 解码前后都执行大小校验，并核对声明 `size`。
- 只允许实际格式与 MIME 一致的 PNG/JPEG/WebP。
- 解码器设置最大宽高和内存分配限制，拒绝超大尺寸与解压炸弹。
- 所有截图重新编码为 PNG，剥离原始元数据；multipart 固定使用 `image/png` 和序号文件名。

### 4.4 归档与请求资源策略

| 资源 | 上限 |
| --- | ---: |
| 描述 | 2000 字符 |
| 截图数量 | 4 |
| 单张截图输入/规范化输出 | 5 MiB |
| 会话归档未压缩总量 | 100 MiB |
| 会话归档压缩后总量 | 20 MiB |
| 会话归档文件数 | 5000 |
| 日志尾部 | 512 KiB |
| 单次请求有效载荷 | 30 MiB |

预览与提交共用同一个 `ArchivePlan` 规则。ZIP 在 blocking worker 中流式写入 `NamedTempFile`，HTTP multipart 通过 `ReaderStream` 读取临时文件，不把完整归档堆入内存；提交时重新检查文件元数据、压缩后大小和总请求大小。

---

## 5. Multipart 契约

固定顺序为：

1. `metadata`：JSON 文本，始终存在。
2. `description`：UTF-8 文本，始终存在。
3. `log`：可选，`text/plain`。
4. `session_archive`：可选，`application/zip`。
5. `screenshot_0..n`：可选，规范化后的 `image/png`。

metadata 使用 `sessionProjectId` / `sessionTaskId`，不上传用户本地 workspace 绝对路径；同时包含用户标识、客户端版本、上报时间、附件标志和计数。

endpoint 与认证复用 metrics 通道：`{metrics_base_url}/api/client-report/feedback` 和 `X-Maling-Report-Key`。HTTP 客户端设置连接超时与请求总超时。诊断写入 `metrics.log`，返回给前端的错误参数不包含 reqwest 原始错误或对客文案。

---

## 6. 错误协议

后端沿用 `CommandErrorVm { code, params }`，前端按 `code` 本地化：

| code | 场景 |
| --- | --- |
| `feedback.disabled` | 当前渠道未启用反馈能力 |
| `feedback.endpoint-unconfigured` | 上报地址未配置 |
| `feedback.validation-failed` | 描述、会话组合或基础输入无效 |
| `feedback.session-not-found` | 项目或任务无法在后端可信状态中解析 |
| `feedback.attachment-invalid` | base64、MIME、图片格式或解码无效 |
| `feedback.payload-too-large` | 截图、归档或整次请求超过资源策略 |
| `feedback.network-failed` | 连接、DNS 或超时失败 |
| `feedback.server-error` | 服务端非成功响应或客户端内部构造失败 |

服务端非成功状态可返回数字 `status` 参数；网络错误的原始字符串只写诊断日志，不进入 command 响应。

---

## 7. 非本期

- panic hook 与下次启动崩溃上报。
- 自动截屏。
- reportId / 工单追踪入口。
- 自动敏感信息过滤；当前通过明确知情提示让用户决定是否提交日志和会话归档。
