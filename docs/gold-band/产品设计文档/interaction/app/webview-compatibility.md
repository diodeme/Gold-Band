# WebView 兼容能力与分级降级

## 1. 目标与边界

Gold Band 继续使用 Tauri 2 提供的系统 WebView。macOS 使用操作系统公开的 WKWebView，不嵌入第二套浏览器内核，也不尝试切换到 Safari Staged WebKit。

兼容能力解决的是“系统 WebView 能否安全启动并承载同一套业务应用”。它不得按 macOS 版本在业务组件中散落特判，不改变 task、run、conversation、workflow 等业务 canonical state。

## 2. 根因与设计方向

旧 Intel Mac 白屏的直接证据是业务入口依赖中的 RegExp lookbehind 在 React 和错误诊断加载前解析失败。形成路径是：HTML 静态加载完整业务入口，而产品没有 WebView 能力门禁、统一能力事实源和现代 CSS 降级边界。

该问题属于启动兼容边界的设计缺失。修复采用独立预检入口、能力派生策略和集中适配层，不对某台机器或某个 WebKit 版本增加补丁分支。

## 3. 数据与事实源

页面启动时同步探测一次 `WebviewCapabilities`，得到不可变的当前 WebView 运行事实。纯函数据此派生 `WebviewFeaturePolicy`。能力探测必须验证功能的实际语义，不能只依赖可能存在实现差异的版本字符串或 API 声明：CSS 自定义属性通过临时隐藏节点的变量继承和计算样式验证，其他现代 CSS 能力继续使用 `CSS.supports()`：

| 策略字段 | 取值 | 含义 |
| --- | --- | --- |
| `tier` | `unsupported / compatible / full` | 总体支持等级 |
| `themeRendering` | `fallback-tokens / modern-css` | 主题颜色实现 |
| `responsiveLayout` | `measured / container-query` | 容器响应式实现 |
| `codeHighlighting` | `plain / wasm` | 代码高亮能力 |
| `visualMaterial` | `solid / native` | 实色或原生模糊材质 |

能力和策略属于 WebView 进程内的瞬时事实：不持久化、不轮询、不写入用户偏好、不进入 React 根 Context。平台、系统版本和 WebKit bundle 版本只用于诊断，不能参与策略分支。

## 4. 启动生命周期

1. `index.html` 只加载不依赖 React/Tailwind 的轻量预检入口。
2. 预检安全探测实际 API，并把策略投影为 `<html>` 数据属性。
3. `unsupported` 不请求业务 chunk，显示结构化升级指引和缺失能力。
4. `compatible/full` 动态加载同一个 React App；chunk 解析或加载失败时仍由预检页展示结构化错误。
5. WebView 环境诊断异步上报，不阻塞业务 App 加载，也不改变能力结论。

稳定错误码为 `webview.capability.unsupported`、`webview.app_chunk.load_failed`、`webview.runtime_facts.unavailable` 和 `webview.code_highlighter.unavailable`。

## 5. 三档体验契约

- `unsupported`：缺失 `:has()`、OKLCH、Grid、自定义属性、ResizeObserver 或 structuredClone 等核心能力；业务 App 不加载。
- `compatible`：核心功能、业务流程、Markdown/GFM 语义和数据结果与完整档一致；颜色混合、模糊材质、装饰动画及 container query 使用集中 fallback。
- `full`：使用现代 CSS、原生 container query 和完整视觉材质。

版本参考仅供支持人员识别环境：最低参考约为 WebKit `613.1.17` / Safari 15.4 / macOS 12.3；完整能力参考约为 WebKit `615.1.26` / Safari 16.4 / macOS 13.3。

## 6. Markdown 与代码高亮

Markdown 继续使用 prompt-kit/Streamdown。项目本地文件链接不使用 lookbehind；上游 `mdast-util-gfm-autolink-literal` 的最小 patch 只替换邮箱边界实现，并复用上游已有的前字符验证，AST 语义不变。patch 由 `patch-package` 在安装后应用，上游正式修复后应删除。

代码高亮使用 `shiki/bundle/web` 的 WASM Oniguruma：首个代码块才初始化，全局合并并发初始化，结果缓存最多 128 项。初始化或 grammar 加载失败时保留代码原文、复制和代码块交互，只取消语法着色，且诊断只记录一次。

## 7. 主题与响应式降级

兼容 CSS 统一由 `webview-compatibility.css` 承担：自定义 `color-mix()` 使用稳定语义 token 回退；composer、dialog、sheet、popover 和 wallpaper overlay 收敛为实色；复杂 filter 和纯装饰动画关闭。业务组件不得读取平台或版本。

状态 badge、提示和结果表面统一由 `status.ts` 投影 `running / success / warning / danger` marker。Theme Contract 为每档状态分别提供 surface 和 border token；现代档继续使用 Tailwind 透明主题色，缺少 `color-mix()` 时由兼容层消费对应 token 并保留原状态前景色。状态 token 不复用文件 Diff 或其他业务领域的颜色。不得依赖 Tailwind 透明 utility 的基础声明作为兼容 fallback，因为该声明会退化为不透明源色，可能使同色文字和图标不可见。

缺失 container query 时，已登记的命名容器使用共享 measured adapter。每个可见容器使用 ResizeObserver，连续变化每动画帧最多发布一次，只在 Tailwind 离散 breakpoint token 变化时改写容器数据属性；不写 React state，不扩大页面订阅。完整档不创建该 observer。

## 8. 诊断与隐私

前端只上报 user agent、有界能力布尔值和派生策略。Rust 在 blocking pool 中读取系统版本事实；macOS 直接读取系统 plist，不执行 shell。`runtime.log` 每次页面启动记录一条结构化摘要，user agent 最多 2048 字符，不采集聊天、prompt、附件路径或页面业务内容。

日志继续使用现有 1024 行有界异步 lossy 队列与 8 MiB/4 份轮转；WebView 诊断不新增队列、线程或同步磁盘 I/O。

## 9. 性能与过度设计评审

启动探测是固定数量 O(1) API 调用；CSS 自定义属性语义探测只挂载一个包含两个子节点的隐藏临时容器，读取两次计算样式后同步移除，不保留 DOM、监听器或状态。诊断异步 best-effort。WASM 高亮按需加载、单例复用且缓存有界。兼容档只观察当前挂载的少量登记容器；full 档没有新增 observer、state 或渲染。无全量扫描、轮询、N+1 I/O、无界缓存、主线程文件写入或长锁。

新增抽象限于能力对象、纯策略、预检入口、兼容 CSS、measured container adapter 和 Shiki 官方 WASM 适配器。没有第二套 React App、平台版本矩阵、持久化能力状态、事件总线或自研 Markdown/语法引擎，复杂度与实际兼容边界相匹配。

## 10. 验收边界

Windows 自动化负责三档能力 fixture、接口测试、类型检查、生产构建和 bundle 审计；它不能替代 WKWebView 613 真机结论。Intel macOS Monterey/WebKit 613 必须使用 DevTools DMG 验证启动、runtime.log 诊断、主业务路径、Markdown/WASM 高亮、弹层和窗口缩放。2026-08-31 的首次用户真机结果发现 `CSS.supports(custom-property, value)` 会假阴性并被错误拦截，现已改用语义探测；后续真机已确认应用可以进入业务界面，并发现 Tailwind 透明状态背景退化为不透明源色。该状态表面问题已由公共 marker 和主题软表面 fallback 修复，仍需用修正版 DMG 在同一设备确认成功、警告和错误提示的图标及文字可见。真机复验完成前不得宣称已完全验证。
