# Profile 动态模板开关实施方案

## 目标

让同一个 profile 可以同时复用于普通工作流和 AUTO / AI-DYNAMIC 执行面，同时保证普通自定义角色继续按纯 Markdown 使用，不因模板语法产生隐式行为。

## 设计结论

这是 profile 缺少执行上下文适配能力导致的模型设计缺口，不采用针对 `plan` 角色的运行时代码补丁。统一在 profile 领域增加 `dynamicTemplate` 布尔状态，并由 prompt bundle 在 profile 注入 system prompt 前完成一次受限渲染。

数据与生命周期：

- `ProfileInput`、`ProfileEntry`、前端 `ProfileInput/ProfileVm` 统一持有 `dynamicTemplate`。
- 用户 profile 通过 Markdown frontmatter 的 `dynamicTemplate` 持久化；缺失时为 `false`。
- 内置 profile 由内置 seed 管理开关，不生成用户文件。
- runtime invocation 冻结 profile 正文和开关，确保同一次调用使用一致配置。
- runtime invocation 通过 `PromptExecutionSurface` 显式冻结执行面；普通 workflow 和 AI-DYNAMIC 构造入口分别赋值，不使用 `runtime_node_id` 推断。
- 执行面序列化与模板枚举统一采用 camelCase：`workflow | aiDynamic`。

接口与渲染：

- 角色创建、更新、查询接口使用 camelCase 字段 `dynamicTemplate`。
- 开关关闭时正文原样注入。
- 开关开启时使用 MiniJinja 严格模式渲染一次；保存时对支持的执行上下文进行预校验。
- 可用变量为 `execution.surface`、`execution.can_route_next`、`execution.has_output_contract`、`execution.session_mode`。其中后两项表达当前 turn 是否启用 `InlineControl`，不是 invocation 是否持有供后续 finalize 使用的原始 contract；`PostTurnProjection` 业务 turn 必须为 `false`，避免 Profile 提前要求 artifact。
- 模板错误返回结构化错误码 `profile.dynamic-template-invalid`，前端负责本地化文案。

## 实施项

- [x] 扩展后端 profile 数据、frontmatter 和 API 序列化。
- [x] 在 prompt bundle 增加可选的一次性 profile 渲染。
- [x] 将执行面改为 invocation 显式枚举，移除基于动态节点身份字段的隐式判断。
- [x] 在角色编辑页增加默认关闭的 shadcn/ui Switch 和变量 Tooltip。
- [x] 同步中英文 UI 文案与错误码文案。
- [x] 扫描内置 profile，仅为 `plan`、`dev` 开启动态模板并同步中英文 prompt。
- [x] 增加 profile 持久化、校验、运行时分支和浏览器预览 API 测试。
- [ ] 完成 Rust 测试、前端测试、类型构建和浏览器级交互验收。

## 验收矩阵

| 场景 | 验证 | 预期 |
| --- | --- | --- |
| 新建普通角色 | 打开新增角色表单 | 动态模板默认关闭 |
| 自定义角色开启模板 | 保存合法条件模板并重新读取 | 开关和正文完整保留 |
| 非法模板 | 开启模板并引用未知变量后保存 | 返回 `profile.dynamic-template-invalid` |
| 普通工作流 | 渲染开启模板的 profile | 使用 `execution.surface=workflow` 分支 |
| AI-DYNAMIC | 渲染开启模板的 profile | `InlineControl` turn 使用 `execution.surface=aiDynamic + can_route_next=true`；`PostTurnProjection` 业务 turn 为 `false`，路由协议只在隐藏 finalize 生效 |
| 关闭模板 | 正文包含 MiniJinja 标记 | 标记保持原样，不进行解释或校验 |
| 内置 plan | AUTO / AI-DYNAMIC 中规划完成 | 不等待第二次确认；实现型目标继续安排开发节点 |
| 内置 dev | AI-DYNAMIC 分配 main/worktree/readonly | 遵循 runtime workspace，不额外要求分支确认 |

## 验证命令

```powershell
cargo test profiles
cargo test --test provider_prompt_bundle
cargo test --test ai_dynamic_node
npm run web:test
npm run web:build
```

UI 验证需要启动 `npm run web:dev`，进入“上下文管理 → 角色管理”，检查新建默认值、提示变量、内置角色开关和保存错误展示；验证完成后关闭开发服务器。
