# WebView 堆诊断入口开发方案

## 目标

为 Windows 开发版提供无需打开可见 DevTools 的 WebView2 CDP 入口，使 6021 帧现场和后续真实会话能够稳定执行 baseline / target / final 堆回归；正式版不得增加远程调试攻击面。

## 实施范围

### 数据与接口

- [x] 定义唯一的 `WebviewHeapDiagnosticVm`，统一 PID、回环地址、动态端口、discovery URL 和标记路径。
- [x] 定义 `{ code, params }` 结构化诊断错误。
- [x] 新增只读 Tauri 命令 `get_webview_heap_diagnostic`，直接投影 Core 托管状态。
- [x] 通过编译条件同时裁剪初始化、托管状态和命令注册，正式构建不暴露入口。

### WebView2 启动与标记生命周期

- [x] 在 WebView 创建前通过 `127.0.0.1:0` 申请动态端口。
- [x] 通过 Tauri `additionalBrowserArgs` / WebView2 COM 开启 CDP，并保留已有参数、`--expose-gc` 及 Wry 默认安全参数。
- [x] 原子写入渠道存储根目录下的 `diagnostics/webview-cdp.json`。
- [x] 正常退出时进行所有权比对后清理标记；异常退出遗留标记由下一次启动替换。

### 固化测试

- [x] 开发版/正式版及 Windows/非 Windows 启用矩阵测试。
- [x] loopback-only 与动态非零端口测试。
- [x] 标记可发现性及接口序列化契约测试。
- [x] 多开发实例标记替换安全测试。
- [x] 现有 WebView2 参数保留测试。

### 真实环境验证

- [x] Rust 单元测试、格式检查、debug/release 桌面编译通过。
- [x] 重启 `npm run dev` 后读取标记并访问 `/json/list` 成功。
- [x] CDP target 可在不打开 DevTools UI 的情况下返回 V8 指标。
- [x] 针对真实会话采集 baseline / target / final 快照，并用 memlab 验证 detached DOM、CodeMirror 和累计字符串释放情况。

## 验收边界

本阶段只建设诊断基础设施，不把抓堆操作放进用户 UI，也不把诊断通道带入正式版。若堆回归仍显示 retained objects，后续修复必须以 retainer chain 为证据修改对应生命周期设计，并补充接口级回归测试，不能针对单次快照做对象名硬编码清理。
