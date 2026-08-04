# 会话启动性能与 MCP 健康检查解耦开发计划

## 目标

删除新建会话前约 4～5 秒的重复 MCP 健康检查，并让聊天界面在 ACP session 元数据到达前即可展示，同时保留 MCP 显式诊断能力。

## 实现项

- [x] 用纯配置接口替换会话启动路径中的 MCP 健康过滤接口。
- [x] Worker invocation 和 App ACP 调用统一消费已启用 MCP 配置。
- [x] 增加 invocation、MCP 解析、adapter、initialize、`session/new`、首次 ready update 和创建命令耗时日志。
- [x] 增加 `initializing` 会话壳状态，仅由当前运行节点启用。
- [x] 初始化状态使用正常 chat/composer 布局，历史会话切换保留全屏 loading。
- [x] 增加后端纯配置边界单元测试。
- [x] 增加前端初始化/切换状态机单元测试。
- [x] 完成 Rust 与前端测试、类型检查。
- [x] 启动前端并通过会话 deep link 验证布局和交互。

## 接口验收

1. `configured_acp_mcp_servers()` 对不存在的 stdio 命令不做进程启动，仍返回序列化配置。
2. `enabled = false` 的配置不出现在 ACP 参数中。
3. 当前运行节点无 session 时，即使初始加载标志为 true，也进入 `initializing`。
4. 非当前节点或普通会话切换继续进入 `loading`。
5. 初始化失败或中断继续展示终态，不被初始化壳覆盖。

## 验证命令

- `cargo test configured_acp_servers_include_enabled_entries_without_health_checks`
- `cargo test`
- 前端 ACP session shell 单测。
- 前端 TypeScript 类型检查和生产构建。
- 启动桌面前端后 deep link 到新建会话，确认聊天壳立即出现、初始化状态位于 composer、session 配置在元数据到达前不可操作。

## 完成标准

- 新建会话关键路径不再出现 MCP health preflight。
- 性能日志能独立定位 adapter、initialize 和 session/new 延迟。
- 自动化测试通过。
- UI 实机验证通过且测试资源已清理。
