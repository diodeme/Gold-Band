# WebView 堆诊断入口

## 背景

Gold Band 的桌面会话、文件查看器和编辑器运行在 WebView2 Renderer 进程中。任务管理器只能证明 Renderer 占用较高，不能回答内存由活动对象、待回收对象、detached DOM、编辑器实例还是累计字符串持有。打开可见 DevTools 还会引入 DevTools 自身对象和面板开销，改变被测现场。

因此开发版需要一个不打开 DevTools UI 的稳定诊断入口，供 Chrome DevTools Protocol（CDP）客户端读取 V8 指标和堆快照。该入口只提供调试通道，不在产品界面暴露操作入口，也不负责解释或上传堆数据。

## 设计判断

问题不是缺少一个“抓快照按钮”，而是桌面运行时没有可自动发现、可重复连接、且与正式版安全边界隔离的 WebView 诊断能力。补一个固定端口或长期开放远程调试会形成端口冲突和正式版攻击面，因此采用以下根因设计：

1. 诊断能力由 Rust Core 在 WebView 创建前建立，生命周期与当前桌面进程一致。
2. 端口由操作系统动态分配，只绑定 IPv4 loopback `127.0.0.1`。
3. 通过 Tauri window 的 `additionalBrowserArgs` 将参数传给 WebView2 官方 `AdditionalBrowserArguments`，保留已有浏览器参数及 Wry 默认安全参数。
4. Rust Core 同时持有唯一诊断状态；磁盘标记与 Tauri 只读接口投影同一数据结构，避免两套状态漂移。
5. 整个模块、命令注册和初始化均受 `debug_assertions + Windows` 编译条件约束，正式构建不注册命令、不写标记，也不开放 CDP 端口。

## 数据结构

`WebviewHeapDiagnosticVm` 是唯一公开快照：

| 字段 | 含义 |
| --- | --- |
| `schemaVersion` | 标记与接口契约版本，当前为 `1` |
| `appPid` | 创建该诊断入口的 Gold Band Core 进程 PID |
| `host` | 固定为 `127.0.0.1` |
| `port` | 启动时由操作系统分配的非零端口 |
| `discoveryUrl` | CDP target discovery 地址，格式为 `http://127.0.0.1:{port}/json/list` |
| `markerPath` | 当前标记文件的绝对路径 |

错误统一为 `{ code, params }`，使用 `webview-diagnostic.*` 稳定错误码；Rust 不返回对客文案。

## 生命周期

1. `main` 完成渠道存储目录配置，并解析桌面工作区。
2. 在创建 Tauri/WebView2 前，Core 绑定 `127.0.0.1:0` 获取动态端口，释放预占 listener，并把 remote-debugging 参数写入 Tauri window 配置。Wry 会将该配置直接交给 WebView2 COM `AdditionalBrowserArguments`；不能依赖进程环境变量，因为 Wry 会为 WebView 环境显式提供参数。
3. Core 通过原子写生成渠道隔离的 `diagnostics/webview-cdp.json` 标记；崩溃遗留标记会在下一次开发版启动时被原子替换。
4. Tauri 托管同一份只读状态，`get_webview_heap_diagnostic` 返回内存快照，不重新读取或修改磁盘。
5. 正常退出时，仅当磁盘标记的 PID、端口及完整内容仍属于当前进程时才删除。若另一个开发实例已经替换标记，旧实例不得删除新标记。

## 安全边界

- CDP 具有执行页面脚本和读取页面数据的能力，因此仅允许开发构建启用。
- `host` 不允许配置为局域网或公网地址；端口不允许硬编码。
- 正式构建不存在 Tauri 命令分支，也不设置 WebView2 remote-debugging 参数。
- 开发版覆盖 `additionalBrowserArgs` 时必须显式保留 Wry 默认的 SmartScreen/OOUI 禁用项和 autoplay 策略，并保留配置中已有参数。
- 标记仅用于本机诊断发现，不包含业务数据、会话内容或凭据。
- 诊断客户端负责把堆快照写入开发者指定目录；Gold Band 不自动上传或长期保存快照。

## 回归验收

接口测试固定以下不变量：

- 只有 Windows debug 构建满足启用条件；
- 分配端口非零，浏览器参数明确绑定 `127.0.0.1`；
- 原有 WebView2 参数（包括 `--js-flags=--expose-gc`）保持不变；
- 标记 JSON 与只读接口使用同一序列化契约；
- 旧进程清理不会删除被新进程替换的标记。

真实诊断使用 baseline / target / final 三阶段：进入目标会话后抓 baseline，复现流式会话和文件侧栏操作后抓 target，关闭资源并触发 GC 后抓 final。快照通过 memlab 分析，不手工读取 `.heapsnapshot`；重点核对 detached DOM、CodeMirror 实例和累计消息字符串是否在 final 中释放。
