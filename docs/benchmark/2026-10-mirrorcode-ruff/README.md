# MirrorCode Ruff（Go）：GPT 5.6 sol 上的 Gold Band AUTO vs Claude Code（2026-10）

本组实验独立于 [mailauth 实验](../2026-10-mirrorcode/README.md)，模型和任务都不同，不合并统计。

## 1. 实验配置

| 项 | 值 |
| --- | --- |
| 任务 | MirrorCode `ruff`（L 档，参考实现 Rust 约 250k LoC，424 条规则；可见 761 例 / 隐藏 61 例） |
| 实现语言 | Go（与论文一致） |
| 模型 | `anthropic/gpt-5.6-sol`，订阅端点，经 Inspect anthropic provider |
| effort | high（Inspect 0.3.276 对非 Claude 4.7+ 模型名会把 `xhigh` 降为 `high`，见 `anthropic.py:1298`） |
| 上下文 | CC 侧 `inspect[1m]`，1M 窗口，默认 autocompact |
| 预算 | 不设 token 上限（`TOKEN_LIMIT` 未设置），墙钟上限 7 天 |
| 沙箱 | workspace 8g 内存，`--no-sandbox-cleanup` 保留容器 |
| Runner | `run-gpt.sh` + `env-gpt.sh`（密钥只在 WSL 的 `gpt.env` 中，权限 600），任务代码 `gb_mirrorcode.py` 与 mailauth 相同 |

冒烟测试（`rev`，Go）：两臂都是 exit 0，均通过 205/208（隐藏 52/52）。

## 2. 与官方设置的差异

- **没有提交闸门**：两臂都是 `gated_submit=None`，由 agent 自己决定何时结束。论文的官方 agent 会锁住 `submit`，直到满足以下任一条件：可见用例全过、用掉 30% 预算后分数不再提升、预算即将耗尽，或连续 100 轮不调用工具。论文指出，不加闸门时模型普遍提前提交。
- 没有 `evaluate_testcases` 工具；agent 只能用工作区里的 `test_cases/cases.jsonl` 和参考二进制自测。
- 论文 L 档预算为 10B token；本组不设上限。

## 3. CC 无闸门单会话（cc-ungated-r1）

| 指标 | 值 |
| --- | ---: |
| 可见 | 336/761（44.15%） |
| **隐藏** | **5/61（8.20%）** |
| 全部 | 341/822（41.48%） |
| 总 token | 309,442,360 |
| 非缓存 input / output（含 reasoning 81,398） / cache read | 873,422 / 244,330 / 308,324,608 |
| Eval 开始 → 结束（台北时间） | 2026-10-08 11:25:33 → 19:27:51（8小时2分） |
| 状态 | `success`，exit 0，无 sample error/limit |
| 会话 | 1,090 turns，子 agent 3 次，compaction 0 次，主会话上下文峰值约 507K / 1M |

**结束原因：已知未完成，主动收工。** CC 自写的差分脚本在结束前 10 分钟显示可见用例逐字一致 335/761，这和官方可见分 336/761 基本相同。它当时仍在以每 10 分钟约 8–10 例的速度提升，没有碰到上下文、token 或时间限制。最终总结只列了已实现的功能和"已对 761 例做过大量测试"，没有说明通过率。这正是论文用提交闸门防范的提前提交。

**耗时构成**：模型等待约占 89%，工具执行约占 13%（跑对比测试 0.69 小时，`go build` 0.25 小时）。每个 turn 平均约 23 秒，平均输出约 220 token，瓶颈是长上下文请求的单轮延迟，不是编译。

论文对比：Ruff 是全基准最难的目标，最好的运行隐藏用例只有 67%，没有任何运行达到 99%。本次 8% 是在无闸门、约 3% 论文预算下的结果，不能直接和论文数字比较。

原始材料：`G:\gb-bench\archives\mirrorcode-ruff-gpt\cc-ungated-r1\`，包含 `.eval`、console、agent archive（含 CC 会话与 git bundle）、scorer 导出源码、逐例评分以及 runner 快照，每个文件都有 SHA-256 校验。manifest 同步保存在 [results/cc-ungated-r1.json](results/cc-ungated-r1.json)。容器 `inspect-mirrorcode-c-in2mzrt-*` 已保留。

## 4. AUTO 第一轮：运行时异常暂停导致提前结束（auto-r1-paused）

| 指标 | 值 |
| --- | ---: |
| 可见 | 123/761（16.16%） |
| 隐藏 | 1/61（1.64%） |
| 全部 | 124/822（15.09%） |
| 总 token | 637,609,395 |
| Eval 开始 → 结束（台北时间） | 2026-10-08 11:25:33 → 2026-10-09 01:21（约 13小时56分） |
| Gold Band 退出码 | 2，run `paused`，`pause_reason=runtime-abnormal` |

这次结束不是评测能力的结果，而是 Gold Band 运行时的故障链：

1. foundation-dev 在 2026-10-09 01:18 以 `end_turn` 完成了工作，总结为"已实现共享的 Ruff 兼容 Go 基础层"，但没有输出节点 artifact。此时这个会话的上下文约 920K。
2. Gold Band 在同一会话里发出 artifact 提醒，即输出协议重申 prompt，内容包括运行时上下文。
3. 加上这条 prompt 后，请求超过了上游模型实际的上下文窗口，上游返回 `invalid_request_error: Your input exceeds the context window of this model`。经过 bridge 和 CC 后，这个错误变成了 "API returned an empty or malformed response (HTTP 200)"；ACP 返回 `-32603`。
4. Gold Band 把它归类为 `provider.acp-prompt-failed`，`recovery=manual`，暂停了整个 run。无头运行时没有人接手，CLI 以退出码 2 结束，Inspect 随即按当时的工作区评分。

结果是只评到了 foundation 阶段的代码：基础层加 14 条首批规则，其中按这 14 条规则自测时可见 747/761 一致。recon-plan 规划的规则族 fan-out（任务 2–8）和集成收敛（任务 9）都没有执行。所以这个分数不能代表 AUTO 的编排能力，只能作为一次运行时故障的记录。

根因归属（按请求数据核对）：
- **测评配置窗口不匹配（根因）**：被拒的请求由约 920K 会话、约 5K token 的提醒（15,867 字符）和 `max_tokens=32000` 组成，总计约 960K，上游以 `Your input exceeds the context window` 拒绝。可见 gpt-5.6-sol 的实际窗口小于约 960K，而测评按 `inspect[1m]` 告诉 CC 窗口是 1M。CC 的 autocompact 要接近 1M 才触发，所以没有提前压缩。在这种会话里，任何普通追加消息都会被拒，这不是 Gold Band 特有的行为。
- **Inspect bridge 改写了错误（放大因素）**：上游的上下文超限错误经过 bridge 变成了 "API returned an empty or malformed response (HTTP 200)"。CC 识别不出 prompt-too-long，所以本可以先压缩再重试的被动恢复也没有触发。
- **Gold Band 恢复策略（次要）**：节点 provider 报错时整个 run 被标为 `recovery=manual` 并暂停，在无人值守下直接结束。这一点可以改进，但不是根因。
- 后续运行应把 CC 的 `CLAUDE_CODE_AUTO_COMPACT_WINDOW` 设为上游实际可用值以下，两臂都要设。闸门版 CC 用的是同样的 `[1m]` 配置，会受到同一个上限约束。

原始材料：`G:\gb-bench\archives\mirrorcode-ruff-gpt\auto-r1-paused\`，manifest 见 [results/auto-r1-paused.json](results/auto-r1-paused.json)。容器 `inspect-mirrorcode-g-ikoexfd-*` 已保留。

## 5. 进行中

- **AUTO（ruff-go-gpt-auto）**：2026-10-08 11:24 启动。recon-plan 把工作拆成 9 个任务，其中任务 1（共享基础：CLI、Python parser、语义层、lint 内核、差分框架）是单节点硬前置，完成后才会展开规则族的 fan-out。
- **CC 闸门版**：`TOKEN_LIMIT=10B`（论文 L 档预算），目的是得到一个和论文口径一致的 CC 基线。注意它比 AUTO 和 CC 无闸门多了两个条件：提交闸门和官方评测工具。所以它只作为论文口径的参照，比较编排能力仍以 AUTO 对 CC 无闸门为准。
  - 第一次（`ruff-go-gpt-cc-gated`）于 2026-10-08 21:10 启动，22:58 人为中止，没有成绩。中止时官方可见分 199/761，容器 `inspect-mirrorcode-c-ioagd2m-*` 保留。中止原因：Inspect 默认把整个 sample 的 transcript 留在内存里，每次模型调用的完整输入都各存一份，sample 结束时才去重写盘。CC 无闸门那次进程最高约 4 GB，落盘 sample 只有 56 MB。和 AUTO 同时跑会挤占主机内存，所以改为开启有界模式 `INSPECT_TRANSCRIPT_BOUNDED=1` 后重跑：内存只保留最近 100 条事件，更早的事件转存到 SQLite。AUTO 启动时没开这个模式，运行中改不了。
  - 重跑（`ruff-go-gpt-cc-gated-b`）开启了有界模式。实现在 `gb_mirrorcode_gated.py`，离线测试在 `test_gated.py`：
  - 官方 `evaluate_testcases` 通过 Inspect `bridged_tools` 以 MCP 形式交给 CC，每次调用都把成绩记进官方 `SubmissionGate`；
  - CC 结束会话时，由官方 `submit_allowed` 判断能否提交（`gated_submit=0.3`）；仍锁定时，用 `claude -p --continue` 加官方续跑消息 "Please proceed to the next step using your best judgement." 续接同一会话；
  - 需求里关于 `submit` 的句子由官方原文改写为"结束会话即提交"，上游原文一旦变化就直接报错；
  - 与论文的偏差："连续 100 轮不调用编辑工具"改为"连续 3 次结束会话且 `src/` 无改动"后放行。
  - 冒烟（`rev`，50M 上限）：MCP 已连接，调用了 4 次评测，可见达到 100% 后闸门解锁，最终 208/208。agent 读到的 `best_visible=1.0` 正是工具写进去的值，说明闸门和工具读写的是同一个 sample store。
  - 续跑分支探针（`gb_resume_probe.py`）：环境与闸门版相同，用的也是同一个 `_cc_gated_command`。第一次会话记下一个暗号，第二次走 `--continue` 续跑命令时 CC 准确答出了这个暗号；两次 session id 相同，MCP 都处于已连接状态，退出码按 `0 0` 追加。
  - 离线测试（`test_gated.py`，共 11 个）用 Ruff 参数（10B、1M 窗口）把官方 `submit_allowed` 的锁定和解锁条件都固化了下来，包括：未评测时锁定、可见未满且仍在进步时锁定、可见 100% 时解锁、30% 预算内无进步时解锁、预算将尽时解锁。
  - **运行中改配置（2026-10-09 08:26:40 台北时间）**：主会话上下文到了 912K，而上游实际窗口约 922K（流式二分实测：918,756 可接受，925,006 被拒）。在容器内 CC 的 `settings.json` 写入 `env.CLAUDE_CODE_AUTO_COMPACT_WINDOW=850000`，然后结束 claude 进程（退出码 143）。闸门判定仍锁定，循环用 `--continue` 在同一 session 中续接，08:29:53 完成第 1 次压缩，之后正常推进（08:40 左右官方可见分 408/761）。之后的会话都受这个设置影响，成绩解读时需要注明。
  - **迁移到新 Inspect（2026-10-09 09:26 → 09:28 台北时间）**：为了带上 bridge 超限修复（`bridge_overflow.py`），并撤掉 850K 设置、改回默认压缩窗口，gated-b 被 SIGINT 中断（已用 621,826,639 token，耗时 9:47:12，Inspect 记为 interrupted，没有 sample 成绩），然后在新 eval `ruff-go-gpt-cc-gated-resume` 中继续：
    - 冻结容器 iixynkg 中的 `/workdir`、CC home（含会话）、日志、需求与续跑消息，原路径还原到新 sample，并删除 CC 的 `settings.json`。
    - 官方闸门状态由 `gated_resume_seed.py` 从 sample 缓冲区重建：用 56 次评测的 score 事件，加上评测时刻的累计 token（模型事件按 uuid 去重后求和，结果与 Inspect 汇总值一致），交给官方 `record_visible_score` 回放。得到：最好可见 0.5388（410/761），最好分与最近评测都在 565,402,223 token 时；`_score_call_idx=56`；会话 3 次；连续无改动退出 0 次。
    - 新 eval 中闸门按"已用 + 本 eval 用量"对照 10B 判定，token 标记换算到本 eval 坐标；Inspect 的 sample token 上限设为 10B − 621,826,639。首个会话直接 `--continue` 同一 session（`034cdbe7`）。
    - 中断时进行中的那一轮请求丢失，代价约为一次请求。gated-b 的 runner 已冻结在 `mirrorcode-agent/frozen/cc-gated-b/`，与启动时记录的哈希一致。
    - 记账：两段 eval 相加；最终成绩取续跑 eval 的评分。

## 6. AUTO 续跑（auto-resume，进行中）

第一轮在 Gold Band 暂停后结束（第 4 节）。这次不重跑，而是从暂停点原地继续：

- **继续方式**：Gold Band 新增 `gold-band continue --task-id --run-id`（产品文档见 `interaction/cli/cli.md`），走桌面"继续"同一条路径：暂停的 foundation-dev 在自己的 ACP 会话（约 921K）中以 runtime resume 继续，结束后 runtime 补收 artifact 并继续调度动态图。没有手工修改任何状态文件：`runtime-abnormal` 本来就允许显式继续。
- **二进制**：基于第一轮二进制的构建源码快照（`/root/gb-src`，与容器内二进制 SHA-256 一致），只加入 `continue` 入口及其共用驱动的重构；bundle 的其余部分（Node、claude-agent-acp 0.81.2、git）与第一轮逐字节相同。
- **状态迁移**：第一轮 sample 容器（ikoexfd，进程已冻结）中的 `/workdir`、Gold Band home 与日志、`/home/coder`（含 CC 会话）打包为 `resume-state/ruff-go-gpt-auto-r1.tgz`（SHA-256 `99cf5b08…`），在新的 ruff/go sample 中按原绝对路径还原，评分方式与第一轮相同。
- **Inspect 修复**：bridge 转发 provider 错误时，把上游的超限错误（"exceeds the context window" 等）改写为 Anthropic 的 400 `prompt is too long`，让 CC 自己的被动压缩接手，见 `bridge_overflow.py`，离线测试见 `test_resume.py`。CC 的压缩窗口保持默认，不设置 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`。
- **与第一轮的差异**：Inspect 开启了有界 transcript 模式（只影响宿主内存）。
- **记账**：两段 eval 的 token 与耗时相加计算，最终成绩取续跑 eval 的评分。
- 2026-10-09 08:52:38 启动（`ruff-go-gpt-auto-resume`），08:54:30 run 与动态图回到 running。
- 续跑后第一条请求约 921K + 续跑消息，上游拒绝。经过 bridge 改写后，CC 识别为 prompt-too-long，并于 08:57:22 开始被动压缩；压缩请求本身也超限，CC 逐次裁掉最早的历史后重试，09:03:41 完成，上下文 921,126 → 21,588。修复在真实场景中验证有效。
- 09:05 foundation-dev 补交 artifact、proposal 被接受，动态图调度下一个节点 `foundation-hardening`。这是 foundation-dev 自己决定追加的串行打磨节点：首批 14 条规则的自测为 757/761，它要求修到 761/761 再 fan-out，否则继续接串行 hardening。
- **人工改配置（09:20 台北时间）**：串行链每一层都占深度，为了不让 fan-out 被 `maxDepth=10` 卡住，按用户要求把 AI-DYNAMIC 的 `control.maxDepth` 从 10 改为 50。权威来源是 run 的执行计划 `execution-plan/revisions/plan-000001.json`：每次接受 proposal 时，runtime 都会用它覆盖 graph 的 control，然后才做深度检查。所以同时原子修改了它、`workflow.snapshot.json`（09:37）、`graph.json` 和 `dynamic-run.json`（09:20）。核对确认除 maxDepth 外内容完全不变，原文件备份在容器的 `/opt/gold-band/logs/*.before-maxdepth.json`。这次是对 AUTO 配置的人工干预，成绩需要注明。

## 7. 因模型问题暂停（2026-10-09 13:08:35 台北时间）

上游模型出现问题，按用户要求同时暂停两臂，等模型恢复后再续跑：

- 方式与 gated-b 迁移相同：两个 eval 同时 SIGINT（Inspect 记为 interrupted，不评分），趁缓冲区被删除之前，用 SQLite 在线备份到 `resume-state/<job>.buffer.db`。13:09:21 两个 unit 结束后，结束容器内 coder 的全部进程。容器保留，状态保持原样。
- 本段用量：
  - CC `ruff-go-gpt-cc-gated-resume`：135,940,614 token，耗时 3:40:59，累计 757,767,253 token；
  - AUTO `ruff-go-gpt-auto-resume`：218,187,411 token，耗时 4:16:08。
- 续跑前要处理的问题：
  - CC 的 seed 要合并两段（gated-b 加上 gated-resume）的 token 与闸门标记；
  - AUTO 是在进程被杀时暂停的，run 仍记为 running；中断时还有 3 个并行 fan-out 叶子在跑，现有 `continue` 不支持多个叶子同时暂停。

## 8. 反复暂停与继续（分段运行）

订阅账号的 5 小时额度撑不完整个任务，两臂需要反复"跑一段、停一段"。每一段都是一个独立的 Inspect eval，记账和续跑都以分段账本为准。

- **分段账本**：`resume-state/<arm>/ledger.json`，由 `segments.py` 维护。每段记录 job、状态（running / interrupted / completed）、本段 token、checkpoint 和冻结状态。
  - **暂停**：SIGINT 该 eval，不评分。runner 在 teardown 中写 checkpoint，并让 agent 暂停。
  - **关闭**：从 eval 日志判断结束类型，结束容器内残留进程，把状态冻结为 `s<N>.tgz`。段的 token 优先取 teardown 写的 checkpoint；checkpoint 早于本段开始（崩溃段没有 teardown，2026-10-13 断网事故）时，改从冻结状态里的会话记录统计：assistant 消息按 message id 去重（会话续跑会整份重写历史）、按段的时间窗过滤，累计 input+cache read+output。该方法在两臂各一段上与账本核对到个位数一致。
  - **续跑**：从最后一份冻结状态新开 `<base>-s<N+1>`。容器一律保留。
- **CC 闸门版**：
  - 会话异常（退出码非 0、没有新的 result 记录、`is_error`）算作基础设施中断，不算一次提交，抛出 `SegmentInterrupted`，本段不评分。
  - checkpoint 是本段结束时的闸门种子（`ResumeSeed.from_gate`，即 `seed_gate` 的逆运算），下一段直接沿用。
  - gated-b 加上 gated-resume 的链式种子已核对：累计 757,767,253 token，idx 73，最好可见 0.6544（498/761）。
- **AUTO**：
  - 只有 Gold Band 退出码 0 或 1（run 结束）才评分，其余抛出 `SegmentInterrupted`。
  - teardown 先对 gold-band 发 SIGTERM，最多等 120 秒，CLI 会把 run 暂停为 `process-interrupted`；之后才结束 agent。
  - 下一段用 `continue --recover-interrupted` 继续：先把仍记为 running 的 run 按桌面启动恢复的语义暂停，再逐个继续所有暂停的 AI-DYNAMIC 叶子。同一 attempt 只有一个调度 driver。
  - 产品改动见 `interaction/cli/cli.md`。
- **账号看门狗**（`watchdog.py`，决策逻辑在 `watchdog_policy.py`，systemd unit `gb-watchdog`，2026-10-14 起为 `/etc/systemd/system/` 下的持久 unit，`Restart=always`，WSL 重启后自启；安装脚本 `G:\gb-bench\install-persistent.sh`）：每 5 分钟检查一次，节奏模仿人工查看。
  - **额度读取**：读 3 个托管账号的实时额度（Codex `wham/usage`），每个账号单独排期。
    - 余量 >30：每 20–30 分钟随机读一次。
    - 余量 ≤30：每 5 分钟读一次。
    - 某个窗口已用完：在该窗口的 `reset_at` 之后再随机等 1–5 分钟读一次；自动重置后立即重读。
    - 没到时间的账号沿用上次读数参与决策。
  - **指纹**：每个账号每隔 1–2 小时（随机）用 ModelTrace 测一次 `gpt-5.6-sol`（medium 思考强度），首轮在启动后 1 小时内随机进行。额度用完的账号到期也不测，等额度恢复后再补测。
    - 测试请求带账号前缀 `mt-<邮箱前缀>/gpt-5.6-sol`，固定打到该账号；不带前缀的请求照常轮询。
    - 读到 `gpt-5.6-luna` 即隔离该账号：禁用，优先级降到 -100。读到其他非 sol 的结果（如 `gpt-5.5`、`gpt-6-luna`）重测一次，重测仍不是 `gpt-5.6-sol` 也隔离。
    - 被隔离的账号按同样的随机间隔临时启用约 2 分钟复测，读到 sol 才恢复。
    - 3 个账号全部被隔离时停机。
  - **额度**：
    - 运行中合计余量 ≤ 5 时暂停两臂；暂停后任一账号余量 ≥ 30 才继续。
    - 余量只计等待能恢复的窗口：账号还有自动重置次数时只看 5 小时窗口，因为周额度用完会立即重置，不需要等；自动重置次数用完后，周额度也计入。暂停后如果没有任何账号还能达到续跑余量（周额度不够又不能重置），停机，不静默等待。
      - 2026-10-12 修正：此前余量取两个窗口中较紧的一个。三个账号周用量到 72–76% 后余量最多 28，暂停后永远达不到 30；而周额度不用完又不会触发重置，于是卡死。从 14:40 一直停到 21:19，修正后才续跑。
    - 5 小时额度只等待恢复。
    - 某个账号的周额度一用完，就立即用重置机会恢复它（一次重置同时恢复周额度和 5 小时额度），不等其他账号。
    - 每个账号最多自动重置 2 次，并且以服务器返回的剩余次数（`available_count`）为准，始终至少留 1 次给人工。因此人工手动用过的次数也会被算进去。只有 `applicable_available_count > 0`（账号当前被限额、有可生效的重置）时才执行。redeem id 先落盘，保证重试时不会重复扣次数。
    - 所有可用账号的周额度都用完，并且都不能再自动重置时，停机。
  - **分段**：自行结束的段会被关闭并续跑。连续 2 段在 5M token 内结束，视为没有进展，停机。某一臂完成时只报告，不影响另一臂。
  - **网络抖动**：访问 CPA 时遇到网络层错误或 502/503/504，客户端退避重试 3 次（5/15/45 秒），认证错误不重试。连续约 30 分钟（6 次检查）都失败才停机。
  - **2026-10-13 断网事故**：14:53 续跑后遇到 429/502 风暴，两臂 70 分钟没有一次成功的模型调用（账号 5 小时用量保持 0% 证实），CC 会话直接启动失败，段以 0 token 正常关闭续跑，无进度与记账损失。
  - **2026-10-14 WSL 虚拟机重启**：12:42 WSL 弹跳（两臂当时已正常暂停，状态已冻结，零损失；Docker 开机自启无恙），但看门狗与内存回收定时器当时是 systemd-run 临时 unit，没能存活，13:50 额度恢复后无人续跑，16:38 才人工拉起。此后两者改为持久 unit，WSL 重启后自动恢复。
  - **停机**：暂停两臂，等人工执行 `watchdog.py release`。`watchdog.py hold` 可以人工停机。
  - **事件**：写入 `watchdog/events.jsonl`（台北时间）。
  - **范围**：只改动这 3 个托管账号，不碰其他凭据，也不会用到第 3 次重置机会。
  - **2026-10-13 账号范围修正**：一个额外的 Pro 订阅（cloudiam999，前缀 `pro`）10-11 20:10 起意外加入轮询，至 10-13 22:43 承载过 259 个请求（两臂 token 记账不受影响；其模型指纹未被探测，无法追溯）。当晚在其 `excluded-models` 中排除 `gpt-5.6-sol`：两臂全速运行时其请求计数归零（轮询已不含它），显式 `pro/gpt-5.6-sol` 前缀仍通（留给操作者自用）。此后 benchmark 流量只走 3 个托管账号。
- **等 token 结算**：两臂按相同 token 预算比较。
  - AUTO 完成后，以它账本的累计 token 作为预算。CC 若尚未完成，就继续跑。
  - CC 的实时 token（账本累计加上运行段的 sample buffer 中已完成模型事件的 `total_tokens`）达到预算后，看门狗暂停 CC，不再续跑，并对该冻结状态做一次官方评分。
  - **快照评分**：`score-snapshot.sh` 只取冻结状态中的 `workdir/src`，交给 `gb_mirrorcode_snapshot` task（同一 setup、scorer、版本，模型为 mockllm）还原并评分。评分 job 与运行 job 分开，结果写入 `snapshot-scored` 事件。
  - **校验**：用该方法给 CC s2 冻结状态（757,767,253 token）评分，得到 visible 0.6544（与当时 CC 自己的最好可见分 498/761 一致），hidden 0.3115。
  - `watchdog.py lift-budget` 解除预算，CC 可以从冻结状态继续跑。
- **离线测试**：`test_segments.py`、`test_watchdog.py`、`test_cpa.py` 与 `test_sample_buffer.py`，共 89 个。重构后，两份已存的闸门种子由 `gated_resume_seed.py` 重新生成，结果完全一致。
- **离线预演（2026-10-11）**：在同镜像的一次性容器中还原 AUTO s2 冻结状态，断网运行。
  - 第一遍 `continue --recover-interrupted`：3 个叶子都恢复了原 ACP 会话（`restored=true`），断网下 run 正常暂停为 runtime-abnormal。
  - 第二遍运行 20 秒后发 SIGTERM：1 秒内以退出码 2 退出，run 为 `process-interrupted`。
  - 预演中发现并修复了一个产品缺陷：容器 PID 1 不回收孤儿时，Agent 后代变成僵尸，进程组存活判定一直为真，终止时关闭连接会卡死，CLI 无法退出。详见开发计划"Headless runner 反复暂停与继续"。
  - 修复后二进制 SHA-256 为 `6dedb346…`，bundle `mc-agent-bundle-continue2.tgz` 为 `4e3a7fea…`。
