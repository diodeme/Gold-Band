# Gold Band vs agency-orchestrator 对比矩阵

## 一句话结论

Gold Band 和 agency-orchestrator 有明显重合，但不等价。

- `agency-orchestrator` 更像：**多 agent 的 workflow / DAG 执行器**
- Gold Band 更适合定位为：**面向代码任务的、可恢复的、artifact 驱动的 runtime**

---

## 对比矩阵

| 维度 | Gold Band | agency-orchestrator | 结论 |
|---|---|---|---|
| **核心定位** | 面向工程任务的、可恢复的 runtime | 多 agent 的 workflow / DAG 执行器 | **定位不同，重合但不等价** |
| **核心抽象** | `task / run / round / node / attempt` | `workflow / step / dependency / output` | **Gold Band 抽象更强、更像 runtime** |
| **工作流模型** | `worker / exec / verify` + control DSL | YAML step graph + `depends_on` + condition / loop | **AO 更偏 workflow engine，GB 更偏 controlled execution** |
| **控制流语义** | repair loop、acceptance loop、`$end`、`continue / retry / kill` | DAG 执行、条件、loop back、approval step | **Gold Band 的控制语义更细、更工程化** |
| **状态模型** | 区分 `status` 和 `outcome`，有 `invalid` / `paused` / `killed` | 以 step 执行为主，状态模型更轻 | **Gold Band 更适合恢复 / 审计** |
| **artifact 模型** | canonical artifact 是一等公民，attachments 是 side effect | 主要是 step 输出文本 / markdown | **Gold Band 明显更强** |
| **执行真相来源** | runtime 依赖 canonical artifacts 做控制判断 | 多数依赖 step output / context chaining | **Gold Band 更适合“可验证执行”** |
| **verify / 验收** | `verify` 是一等节点，显式证据输入，产出 `verify-result` | 更像普通 review / synthesis step | **Gold Band 更严肃** |
| **exec 语义** | `exec-plan -> exec-result`，显式命令执行层 | 没有 Gold Band 这种 artifact-driven exec 中心 | **Gold Band 差异化强** |
| **provider 边界** | 强调 runtime / provider implementation 严格分层，A()/B() 边界清楚 | connector 更像 prompt-in / text-out 接口 | **Gold Band 设计更完整** |
| **session 模型** | 区分 `run continue`、`retry`、`open-session`、`worker-ref` | resume 更像“复用上次结果继续跑” | **Gold Band 更强** |
| **恢复能力** | attempt 级恢复、invalid 重结算、provider handoff | 主要是 workflow resume / skip completed steps | **Gold Band 更像操作系统，不只是 runner** |
| **inspectability** | 明确 state files、artifact、progress、layout | 有 run output 和 metadata，但更轻 | **Gold Band 更可审计** |
| **CLI 产品形态** | 明确 CLI 是一等公民，插件是增强层 | 已有成熟 CLI，命令丰富 | **AO 现在更领先，GB 设计上不弱** |
| **实现成熟度** | 目前以 spec 为主 | 已经是可用实现 | **AO 当前领先** |
| **适合的用户心智** | “我需要一个严肃、可恢复、可检查的代码任务 runtime” | “我需要一个可用的多 agent workflow 工具” | **用户心智可分开** |
| **风险** | 如果做成泛化 workflow engine，会和 AO 高度重合 | 已经占了通用 orchestration 位置 | **GB 不要泛化过头** |
| **最强差异点** | artifact-grounded、recoverable、inspectable、strict runtime discipline | provider/connectors 丰富、工作流产品化更成熟 | **Gold Band 仍然有空间** |

---

## Gold Band 应该重点强调的标签

建议以后持续强调这 5 个关键词：

- **Contract-driven**
- **Artifact-grounded**
- **Recoverable**
- **Inspectable**
- **CLI-first runtime**

---

## 建议的一句话定位

### Gold Band

> 一个面向代码任务的、以 canonical artifacts 和可恢复控制流为核心的 runtime。

### agency-orchestrator

> 一个面向多 agent 工作流的 DAG / step orchestration engine。

---

## 最重要的判断

如果 Gold Band 继续做成：

- 多 agent
- 多 provider
- YAML / JSON workflow
- prompt role orchestration

那会越来越像 agency-orchestrator。

如果 Gold Band 坚持做成：

- `worker -> exec -> verify`
- artifact 驱动
- attempt 级恢复
- runtime control first
- provider handoff 和 runtime continue 分离

那它就仍然有明显产品边界。

---

## 当前建议

不要把 Gold Band 定位成“另一个 agent workflow 工具”。

更合适的方向是：

> **Gold Band 是一个面向工程任务的、可恢复、可检查、由 artifact 驱动的 AI runtime。**
