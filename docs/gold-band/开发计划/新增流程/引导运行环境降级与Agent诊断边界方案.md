# 引导运行环境降级与 Agent 诊断边界方案

- 日期：2026-11-13
- 状态：已实施（2026-11-13）；2026-11-14 由[首次启动引导开发计划第 23 节](首次启动引导开发计划.md#23-2026-11-14-运行环境改为按-agent-启动方式提示)取代：`agentDetectionReady` / `packageManagerReady` 与 Node 门槛已删除，运行环境只提示不阻断，npx 依赖改为按候选启动方式判定。本文其余内容保留为历史记录。
- 范围：首次启动引导的运行环境检查、Agent 检测入口、Agent 管理页诊断展示边界
- 不包含：自动安装 Node.js/npm、自动修改 PATH、为 Agent Doctor 建立第二套诊断状态

## 1. 问题与根因

当前 `EnvironmentReport.runtime_ready` 只有在 Node.js、npm、npx 全部可用时才为 `true`。引导的“下一步”、Agent 检测入口和最终应用都共用这一硬门槛，因此 npm `.cmd` 冷启动超时、npm/npx 安装身份不一致或 npm engines 元数据异常，会阻止用户进入 Agent 检测，即使本机已有可直接运行的 ACP Agent。

这与实际启动方式不一致：

- CodeBuddy 可通过本机 `codebuddy --acp` 启动，不依赖 npx。
- Claude、Codex 等可以通过本机 ACP adapter 启动。
- 只有 Catalog 选择 npx 启动或需要通过 npx 准备 adapter 的候选才依赖 npm/npx。
- Agent Doctor 已经能够对每个候选完成真实 initialize、session/new 和能力发现，是“当前 Agent 能否运行”的权威证据。

npm 版本范围本身不是主要问题。稳定但未验证的新 major 已经进入 `unverified`，实际工具链和 engines 通过时可继续。需要修正的是“npm/npx 是否对所有 Agent 都构成全局阻断”这一设计。

## 2. 目标与非目标

### 2.1 目标

1. Node.js 仍作为当前版本的引导基础门槛，Node 不可用时继续阻断。
2. npm/npx 继续检测和展示，但不再单独阻断进入 Agent 检测。
3. 本机直接启动的 Agent 在 npm/npx 异常时仍可被发现、诊断和选择。
4. 依赖 npx 的 Agent 由自己的 Doctor 失败决定是否可用，不污染其他 Agent。
5. 至少一个 fingerprint 匹配且 Doctor 健康的 Agent 仍是最终提交门槛。
6. 明确页面边界：全局运行环境归引导页，单 Agent 诊断归 Agent 管理页。
7. 不新增工具扫描、缓存、后台轮询或第二套 Agent 健康事实源。

### 2.2 非目标

- 不因为 npm/npx 异常直接允许“无可用 Agent”的配置提交。
- 不把 npm/npx 的版本、路径和错误详情复制到每个 Agent 卡片。
- 不在引导页提供 Agent 命令、参数、环境变量编辑。
- 不为本次改动支持 Node 完全缺失时的 native Agent 运行；该场景需要另一次产品决策。

## 3. 数据结构

### 3.1 EnvironmentReport

在 `src/onboarding/environment.rs` 的 `EnvironmentReport` 中增加两个派生能力字段，保留现有 `runtime_ready` 表示完整工具链：

```rust
pub struct EnvironmentReport {
    pub checks: Vec<EnvironmentCheck>,
    /// Node + npm + npx 全部处于 runtime-usable 状态。
    pub runtime_ready: bool,
    /// 当前版本允许进入 Agent 检测：Node 处于 Ready 或 Unverified。
    pub agent_detection_ready: bool,
    /// npm 与 npx 都处于 Ready 或 Unverified，且组合校验通过。
    pub package_manager_ready: bool,
}
```

语义：

- `agent_detection_ready = node.check.status.is_runtime_usable()`
- `package_manager_ready = npm/npx 两项均 runtime-usable`
- `runtime_ready = agent_detection_ready && package_manager_ready`
- `agent_detection_ready && !package_manager_ready` 表示“受限运行”，允许进入 Agent 检测并展示警告。
- 三个字段全部从同一组 `checks` 派生，不单独持久化，不读取第二次 PATH。

在 `EnvironmentReport` 上增加按工具查询的私有 helper，避免继续用数组下标表达业务语义：

```rust
fn check(&self, tool: EnvironmentTool) -> Option<&EnvironmentCheck>;
```

### 3.2 状态映射

| Node | npm/npx | agentDetectionReady | packageManagerReady | 引导行为 |
| --- | --- | --- | --- | --- |
| usable | usable | true | true | 正常继续 |
| usable | 任一不可用 | true | false | 警告后继续到 Agent 检测 |
| 不可用 | 任意 | false | 任意 | 阻断在运行环境步骤 |

“usable”指 `Ready | Unverified`。缺失、过低、预发布、超时、启动失败和组合不兼容不是 usable。

## 4. 后端接口与状态转换

### 4.1 引导门槛

在 `EnvironmentReport` 增加：

```rust
pub fn require_agent_detection(&self) -> Result<(), SetupError>;
```

它只在 `agent_detection_ready=false` 时返回 `setup.runtime-incompatible`，并在 params 携带完整报告。替换以下入口的 `require_runtime()`：

- `src-tauri/src/onboarding.rs` 的 `SetupController::checked_environment`
- `src-tauri/src/onboarding.rs` 的 `start_setup_agent_detection`
- `src/onboarding/defaults.rs` 的 `apply`

`require_runtime()` 如无剩余调用则删除。`runtime_ready` 继续作为展示字段和完整工具链能力，不再承担引导唯一门槛。

最终提交仍由现有 `DefaultSetupSelection` 校验、Agent fingerprint 和 Doctor 健康状态共同保护；环境门槛放宽不能绕过 Agent 选择校验。

### 4.2 Agent 检测

`SetupController` 仍在一次环境检测完成后复用同一份 `EnvironmentReport`，不重新执行 Node/npm/npx 命令。

`prepared_adapter` 和 Catalog 启动选择保持现状：

- 有本机 `localLaunch` ACP 命令时直接使用本机命令。
- 需要 npx 时继续走现有 npx cache/doctor 流程。
- 不在发现层增加“npm 失败就跳过所有候选”的分支；否则会漏掉本机 Agent。
- 依赖 npx 的候选若实际失败，只产生该候选的本轮 `unavailable`，不写回 Agent 管理页的 canonical 诊断。

### 4.3 诊断事实源

- 全局工具事实源：`EnvironmentReport`，生命周期只覆盖当前引导会话。
- 单 Agent 事实源：`ProviderDiagnosticSnapshot`，归属 Agent 管理领域。
- 引导可读取、复用或在陈旧成功快照上执行一次受控重诊断；失败不覆盖已有快照。
- Agent 管理页负责持久化诊断、显示失败原因和执行修复。

## 5. 前端状态

### 5.1 类型

`web/src/onboarding/types.ts` 的 `EnvironmentReport` 增加：

```ts
agentDetectionReady: boolean;
packageManagerReady: boolean;
runtimeReady: boolean;
```

### 5.2 detection state

`web/src/onboarding/detection-state.ts`：

- `environment-completed` 仅在 `agentDetectionReady=false` 时进入 `blocked-environment`。
- npm/npx 异常但 Node 可用时保持可进入下一步；报告本身携带 `packageManagerReady=false`。
- `failureEnvironment` 继续识别后端 `setup.runtime-incompatible` 的完整报告。

不新增持久化状态或 TTL；沿用当前一次环境报告生命周期。

### 5.3 文案状态

引导页计算：

```ts
const environmentLimited =
  environmentState.phase === 'ready' &&
  Boolean(environment?.agentDetectionReady) &&
  !environment.packageManagerReady;
```

- `environmentLimited` 时，运行环境步骤显示 warning token 和“部分 Agent 依赖的 npm/npx 不可用”的说明。
- 后续 Agent 步骤顶部继续显示一次简短警告，不重复展示 npm/npx 路径详情。
- 运行环境步骤详情仍允许查看每项状态和原因。

## 6. 页面交互归属

| 交互 | 引导页 | Agent 管理页 | 设置页 |
| --- | ---: | ---: | ---: |
| Node/npm/npx 探测、版本和路径 | 是 | 否 | 否 |
| npm/npx 受限警告 | 是 | 否 | 否 |
| 重新检测运行环境 | 是 | 否 |  |
| Node 安装指引和 npm/npx 修复提示 | 是 | 否 | 否 |
| 发现本机 Agent | 是 | 否 | 否 |
| 引导候选 Doctor | 是，受控的一次性探测 | 是，canonical Doctor | 否 |
| 自动登记引导成功项 | 是 | 展示已登记结果 | 否 |
| Agent 状态、错误原因、帮助入口 | 只消费健康结果 | 是 | 否 |
| Agent 命令/参数/环境变量/显式路径 | 否 | 是 | 否 |
| 手动 Doctor、周期 Doctor、缓存修复 | 否 | 是 | 否 |
| “需重新诊断”状态 | 只参与本轮探测 | 是 | 否 |
| 重新运行引导 | 引导路由 | 否 | 是 |

### 6.1 运行环境步骤

- Node 不可用：保持当前阻断状态，展示具体工具原因、重新检测和安装指引。
- Node 可用、npm/npx 不可用：显示 warning，不禁用“下一步”。
- 所有工具正常：保持当前 ready 状态。
- 文案明确说明“仍可检测本机 Agent；依赖 npm/npx 的 Agent 可能不可用”，不把 npm/npx 失败描述成整个客户端不可用。

### 6.2 Agent 步骤

- 继续只展示 Doctor 成功且配置已保存的 Agent。
- 若环境处于受限状态，在检测状态旁显示一条低干扰 warning，不展开重复工具详情。
- 没有任何 Agent 通过时：
  - 主操作：“重新检测 Agent”。
  - 次操作：“查看运行环境”。
  - 文案同时保留安装、登录、PATH 和多 Node 版本提示。
- 不展示逐 Agent 失败列表、程序选择器或修复表单；这些属于 Agent 管理页。

### 6.3 Agent 管理页

- 不展示全局 Node/npm/npx 版本报告。
- 每张卡片继续展示自己的 `healthy | stale | unhealthy` 状态。
- Agent Doctor 因 npx/npm 失败时，保留原始结构化原因和配置帮助。
- 不在卡片内增加第二套 npm 探针；需要全局修复时可通过设置页的“重新运行引导”进入环境检测。
- 本方案不新增从 Agent 管理页到引导页的自动修复动作，避免页面同时承担配置诊断与全局环境修复。

## 7. 错误与文案

保留现有结构化错误：

- `setup.environment-check-required`：本次报告不存在或已取消。
- `setup.runtime-incompatible`：仅 Node 不可用时由引导门槛返回。
- `setup.probe-timeout`、`setup.runtime-missing`、`setup.runtime-version-unsupported` 等继续作为工具项状态。
- `setup.agent-unavailable`：候选在引导本轮 Doctor 未通过。

新增一条引导 warning 文案，七语言目录同步维护。文案只描述影响范围：

- zh-CN：`npm/npx 不可用，仍可检测本机 Agent；依赖 npm/npx 的 Agent 可能不可用。`
- 其他语言保持同一语义和 Node.js/npm/npx/PATH 等保留词。

## 8. 实施步骤

1. 数据与派生能力
   - 修改 `src/onboarding/environment.rs`：增加字段、helper 和 `require_agent_detection`。
   - 修改 `check_environment` 组装三个 readiness 字段。
   - 更新 environment 单元测试。

2. 后端入口
   - 修改 `src-tauri/src/onboarding.rs`、`src/onboarding/defaults.rs`，切换门槛到 `require_agent_detection`。
   - 检查 `setup.runtime-incompatible` 参数报告仍完整。

3. 前端状态
   - 修改 `web/src/onboarding/types.ts`、`detection-state.ts`、`OnboardingPage.tsx`。
   - 将 Agent 选择、模型、权限、预览和应用可用条件从 `runtimeReady` 切换到 `agentDetectionReady`。
   - 增加 `environmentLimited` warning 展示。

4. 页面边界
   - 引导页保留运行环境详情和重测入口。
   - Agent 管理页只展示单 Agent 诊断；不复制 npm 探测 UI。

5. 文案与文档
   - 更新七语言 catalog。
   - 更新 `first-run-onboarding.md` 的运行环境章节、验收矩阵和页面边界。
   - 更新本开发计划实施记录和 `agent管理.md` 的边界说明。

## 9. 回归测试

### 9.1 Rust 接口

- Node ready、npm missing：`agent_detection_ready=true`、`package_manager_ready=false`、`runtime_ready=false`。
- Node missing：`agent_detection_ready=false`，`require_agent_detection` 返回 `setup.runtime-incompatible`。
- npm/npx unverified 但联合校验通过：`package_manager_ready=true`。
- 受限环境报告完成：`checked_environment` 成功，Agent 检测可启动。
- 受限环境 + 本机直接启动 Agent Doctor 成功：最终 apply 成功。
- 受限环境 + 无健康 Agent：预览/提交仍被现有 Agent 校验拒绝。
- 环境报告缺失或取消：仍返回 `setup.environment-check-required`。

### 9.2 Agent 发现

- CodeBuddy localLaunch 可用、npm/npx 不可用：候选仍进入 Doctor，成功后可发布。
- 仅 npx 候选、npx 不可用：该候选本轮不可用，不产生全局阻断。
- 已配置且 fingerprint 匹配健康 Agent：继续直接复用，不重复 Doctor。
- 陈旧成功快照：继续使用 2026-11-12 的受控重诊断规则，失败不覆盖管理页快照。

### 9.3 前端 DOM

- Node 不可用：下一步禁用，显示阻断原因。
- Node 可用、npm/npx 失败：下一步启用，显示 `environmentLimited` warning。
- 受限环境进入 Agent 步骤：显示简短 warning，不重复三项工具路径。
- 无 Agent：展示“重新检测 Agent”和“查看运行环境”。
- Agent 管理页：stale 显示“需重新诊断”，healthy 显示“正常”，不出现全局 npm 工具卡片。

### 9.4 既有回归

- `cargo fmt --all -- --check`
- `cargo test -p gold-band --lib onboarding:: --offline -j 1`
- `cargo test -p gold-band-desktop --offline -j 1 <目标测试>`
- `cargo check -p gold-band-desktop --offline -j 1`
- `npm run web:test -- web/tests/onboarding.test.tsx web/tests/agent-diagnostic-status.test.tsx web/tests/i18n-loading.test.ts`
- `npx tsc -p web/tsconfig.build.json --noEmit`

## 10. 兼容与发布

- `EnvironmentReport` 只存在当前 setup controller 的内存中，不持久化，不需要迁移。
- `ProviderDiagnosticSnapshot` 格式不变，Agent 管理页契约不变。
- 本次是开发阶段的产品语义调整，旧“npm 全局不可跳过”规则直接替换，不增加兼容开关。
- 发布说明应明确：运行环境步骤现在允许在 npm/npx 受限时继续检测本机 Agent，但最终仍需至少一个健康 Agent。

## 11. 过度设计与性能复核

不新增：

- Node 版本扫描或目录枚举；
- npm/npx 后台轮询；
- 第二个 Agent Doctor 事实源；
- 持久化环境报告或 TTL；
- Agent 页内重复工具探针；
- 自动修改 PATH 或全局 npm 配置。

性能保持不变：环境检查仍是固定四项并行短进程；受限状态下只多一次从同一报告派生布尔值。Agent 检测仍受现有最多 4 并发、单 Agent 单飞和 3 分钟预算约束。正常健康 Agent 不增加 Doctor；只有未被环境门槛排除的候选继续使用既有流程。

## 12. 实施结果

- 后端：`EnvironmentReport` 已增加 `agent_detection_ready` 与 `package_manager_ready`；引导、默认配置应用和 Agent 检测统一使用 `require_agent_detection`。
- 前端：`environment-completed` 按 `agentDetectionReady` 决定是否阻断；运行环境步骤与 Agent 步骤显示受限 warning，下一步及后续选择改用 `agentDetectionReady`。
- 页面边界：全局 Node/npm/npx 状态和重测保留在引导页；Agent 管理页继续只负责单 Agent Doctor、配置与修复。
- 回归：`cargo test -p gold-band --lib onboarding:: --offline -j 1` 为 42 项通过、2 项按设计忽略；Desktop `npm_timeout_is_a_limited_capability_and_does_not_block_agent_detection` 通过；`web/tests/onboarding.test.tsx`、`i18n-loading.test.ts`、`agent-diagnostic-status.test.tsx` 共 47 项通过；TypeScript 检查通过。

## 13. 验收标准

1. npm/npx 异常不再单独阻止进入 Agent 检测。
2. Node 不可用仍会阻止进入。
3. 本机直接启动的 Agent 可在受限环境下被发现和选择。
4. npx 依赖 Agent 的失败只影响该 Agent，并在 Agent 管理页保留原因。
5. 最终提交仍要求至少一个 fingerprint 匹配且 Doctor 健康的 Agent。
6. 引导页负责全局环境，Agent 管理页负责单 Agent 诊断和修复，不出现重复入口或第二事实源。
