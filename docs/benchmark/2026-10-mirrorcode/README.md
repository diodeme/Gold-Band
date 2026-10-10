# MirrorCode mailauth：Gold Band AUTO vs Claude Code（2026-10）

## 1. 归档范围与结论

本记录已归档两臂各两次完整运行。原始数据的机器可读摘要见 [results/pair-01.json](results/pair-01.json) 与 [results/pair-02.json](results/pair-02.json)。成绩以 Inspect 的官方 MirrorCode scorer 输出为准，不使用 agent 自报成绩。用户最终要求总计三轮（不跑第四轮）；第三轮因保守预算门槛暂未启动，见第 7 节。

首轮 AUTO 比 CC 多通过 10 个隐藏用例，但耗费更多 token 和时间；第二轮 CC 全满分，AUTO 仍差 1 个隐藏用例且耗费更多 token 和时间。**目前只有两对完整运行，表现有波动，不能证明某一方案稳定领先，也不能将差异唯一归因于编排。**

### 首对完整运行（pair-01）

| 指标 | Gold Band AUTO | 纯 Claude Code |
| --- | ---: | ---: |
| 本地 job | `auto-mailauth-v2` | `auto-mailauth-cc` |
| 可见用例 | 851/851（100%） | 851/851（100%） |
| 隐藏用例 | 694/702（98.8604%） | 684/702（97.4359%） |
| 全部用例 | 1545/1553（99.4849%） | 1535/1553（98.8410%） |
| 全部用例 solved@100% | 0 | 0 |
| 全部用例 solved@99% | 1 | 0 |
| 隐藏用例 solved@99% | 0 | 0 |
| 总 token（含缓存读取） | 407,925,598 | 336,549,690 |
| 非缓存 input | 1,873,102 | 717,931 |
| output | 2,295,056 | 863,311 |
| cache read | 403,757,440 | 334,968,448 |
| 模型事件数（不等同于计费请求数） | 1,768 | 711 |
| Eval 开始，北京时间 | 2026-10-07 13:15:02 | 2026-10-07 19:27:21 |
| Eval 结束，北京时间 | 2026-10-07 17:09:39 | 2026-10-07 22:09:27 |
| Eval 耗时（含 setup、收尾及评分） | 3 小时 54 分 37 秒 | 2 小时 42 分 06 秒 |
| agent exit code | 0 | 0 |
| Inspect sample error / limit | 无 / 无 | 无 / 无 |

- AUTO 比 CC 多消耗 **71,375,908 token（约 21.21%）**；不支持“更省 token”的说法。不同 token 类别价格不同，未取得对应完整账单前不把 token 比例等同于费用比例。
- 隐藏通过率高 **1.4245 个百分点**。失败数从 18 降至 8，但单次结果尚不能反映方差。
- token 取自 `log.stats.model_usage`，包含子任务等整次 eval 的模型消耗；不能用部分已完成节点的 ACP 统计代替。
- 首对两臂在不同时段运行，延迟、缓存与服务端负载不是严格受控变量。此前按容器存活时间估算的“约 5 小时/约 3 小时”不作正式耗时。

### 失败分布

从 scorer 的 `scored_cases_00000` 按子命令汇总，不依据截断的文字解释推断剩余失败：

| 子命令 | AUTO 失败 | CC 失败 |
| --- | ---: | ---: |
| `dkim-verify` | 1 | 11 |
| `arc-verify` | 7 | 7 |
| 合计 | 8 | 18 |

两臂的失败均在隐藏用例中。失败输入和逐例结果只保存在本地私有归档，不注入后续重复实验，也不根据它们修改候选实现后混入原始成绩。

## 2. 实验配置与可比性

- 样本：`mailauth_python`，851 个可见用例、702 个隐藏用例，任务版本 `0.1.193`。
- 归档时 MirrorCode checkout：`5c9d7b00b0c6d7609003e33896cd41d28f092fcb`。
- 模型：DeepSeek V4.1 Flash，Inspect 名称 `anthropic/deepseek-flash`；agent 通过 `inspect[1m]` bridge 别名访问它；effort `high`。
- AUTO：Gold Band `run --auto-config`，`claude-agent-acp@0.81.2`，其依赖 `claude-agent-sdk@0.3.280`；动态节点上限 40、fanout 上限 5、并行度 3、深度 10、group 深度 2、workflow invocation 上限 10、禁用 nested dynamic。
- CC：`claude-code@2.1.280`，`claude -p ... --permission-mode bypassPermissions --effort high --output-format stream-json --verbose`。不加 Gold Band 编排，不人为禁用 Claude Code 自带工具或子 agent；“单主会话”不代表它不能自行调用子任务。
- 两臂属对应发布线，**不声称 SDK/ACP 与 CLI 是完全相同的引擎或只有一个严格受控差异**：各自默认 system prompt、工具封装、PATH、HOME、安装内容也有差别。AUTO 额外装有 Node/ACP（Node 不在 PATH），CC 使用原生 CLI 二进制。
- 两臂均使用全新离线 workspace 容器、`coder` 用户、8GiB workspace 内存上限、相同参考程序/文档/可见测试及官方 scorer。
- token 不设上限；agent 命令超时设为 7 天。不代表 AUTO 自身动态节点等预算无上限。
- `--no-sandbox-cleanup`；退出后先停止 agent 进程，再收集产物。收尾有 cancellation shield 和 30 分钟上限。**当前是退出时收集，不是定期 checkpoint 或已验证的无损续跑机制。**
- 两臂 agent 进程 `oom_score_adj=1000`，与 Inspect 模型代理对齐；这减少代理优先被杀的风险，不保证所有 OOM 情形都能无损恢复。
- Inspect `0.3.276`、Anthropic SDK `1.11.0`、AnyIO `4.15.1`（归档时环境版本）。

### 与官方 agent 设置的差异

1. 去掉 `evaluate_testcases` 和 gated submit；从用户任务中移除调用 `submit` 的句子，两臂都以自身运行退出交付。
2. workspace 内存由默认 2GiB 改为 8GiB。
3. 使用 Inspect sandbox agent bridge；`coder` 不经过 `restricted-shell`，因为其 Landlock 限制与 Bun 不兼容。因此隔离/信任边界与官方默认 agent 不完全一致。
4. AUTO 与 CC 均未获知隐藏用例。公开题面/文档/可见测试相同，但并非官方默认 harness 的直接排行榜提交。

因此这里只给出**本地修改 harness 下使用官方 scorer 的成绩**，不声称已获官方榜单认可。论文“最佳轨迹略低于 99% hidden”不足以推出本次超过历史最佳；99% all 与 99% hidden 也不能混用。需要逐条同口径数据才能比较历史排名。

## 3. 首轮轨迹与先前试跑的区别

AUTO 的完整轨迹：foundation → 四个命令族 fanout → merge → group acceptance → `dmarc-verify` → `auth-results` → final acceptance → end。最终所有可见用例通过。

早先 `auto-mailauth`（非 `auto-mailauth-v2`）在 1B token 处截断，成绩 702/851 visible、546/702 hidden。该试跑是预算受限的不同轨迹，不纳入本表完整运行对；不能从第二次运行证明第一次只需继续就一定能完成。之前“合并丢失 auth-results”的推断混用了作废 JS 试跑的材料，已撤回，不作为产品缺陷证据。

## 4. 原始材料与保全

私有归档根目录：`G:\gb-bench\archives\mirrorcode-mailauth\`。

- `pair-01/manifest.json`：首对元数据、文件大小与 SHA-256；与仓库 `results/pair-01.json` 内容相同。
- `pair-01/<job>/logs/*.eval`：完整 Inspect 日志。
- `pair-01/<job>/logs/artifacts/<id>/workspace_src_00000.tar`：真正送入 scorer 的源码；避免只靠 git bundle 漏掉未提交文件。
- `pair-01/<job>/logs/artifacts/<id>/scored_cases_00000.jsonl`：逐例评分（包含隐藏用例信息，不向新 agent 暴露）。
- `pair-01/<job>/artifacts/mailauth_python.tgz`：agent 日志、运行状态、git bundle；不能单独视为未提交源码的完整备份。
- `pair-01/<job>/console.log`：启动与完成记录。
- `pair-01/runner/`：首对结束后、第二对起跑前的 runner 快照；包含首个 AUTO 运行后新增的 CC 支持，不能冒称为首个 AUTO 起跑时的适配器快照。
- `pair-01/extra-files.json`：scorer 导出物及 runtime bundles 的校验信息。
- `bundles/mc-agent-bundle.tgz`、`bundles/cc-agent-bundle.tgz`：固定运行二进制和依赖；第二对复用同样内容，不重编译或升级。

复制后的原始文件均做 SHA-256 一致性校验。`bench.env`、凭据、原始会话和隐藏测试不写入仓库；仓库仅存统计摘要和本说明。

归档检查时仅 CC 首轮容器仍在。AUTO 首轮容器已在此前 CC smoke setup 的全局 sandbox cleanup 中被删除，虽然运行结束时曾保留；其 eval、scored source 和 agent archive 尚在并已备份。本轮不再执行全局 cleanup，不删除任何现存容器。

## 5. 第二次独立重复实验（pair-02）

用户于 2026-10-07 要求两臂各再跑一次，不先依据首轮隐藏失败修补代码或提示词。

| 臂 | 新 job | systemd unit |
| --- | --- | --- |
| AUTO | `mailauth-auto-r2` | `mc-mailauth-auto-r2` |
| CC | `mailauth-cc-r2` | `mc-mailauth-cc-r2` |

- 新建独立 workspace、HOME、session 和 git 仓库；不传 `--resume`，不挂载首轮产物或隐藏测试分析。
- 保持模型、effort、工具/runner 和 runtime bundles 不变；不设 token 上限，保留沙箱。
- 两臂并行运行。起跑前 WSL 总内存约 23GiB、可用约 22GiB，各 workspace 8GiB；scorer、host 和其他进程仍有资源开销，不能视为完全隔离的性能实验。第二对耗时与首对跨时段运行需分别报告。
- 只读观测；不因中间成绩差而人工补指令、重启、删记录或提前择优终止。结束后追加结果，不覆盖 pair-01。
- 起跑状态见 [results/pair-02-start.json](results/pair-02-start.json)（实际启动核验后生成）。

本次只归档与重复运行，不改变产品代码/设计；复用现有 harness，无新增缓存、队列或产品状态模型。大文件留本地，仓库仅保存小型汇总文件。

## 6. 第二轮正式结果（pair-02）

两臂均正常退出（exit-code 0），Inspect `status=success`、sample error/limit 为空，没有人为干预候选实现。

| 指标 | AUTO | CC |
| --- | ---: | ---: |
| 可见 | 851/851（100%） | 851/851（100%） |
| 隐藏 | 701/702（99.8575%） | 702/702（100%） |
| 全部 | 1552/1553（99.9356%） | 1553/1553（100%） |
| solved@100%（all） | 0 | 1 |
| solved@99%（all） | 1 | 1 |
| 总 token | 699,523,860 | 397,644,826 |
| 非缓存 input | 2,497,853 | 899,171 |
| output | 3,566,423 | 1,334,839 |
| cache read | 693,459,584 | 395,410,816 |
| Eval 开始（台北时间） | 2026-10-07 23:19:16 | 2026-10-07 23:19:16 |
| Eval 结束（台北时间） | 2026-10-08 07:28:21 | 2026-10-08 01:49:08 |
| Eval 耗时（含 setup/收尾/评分） | 8小时9分5秒 | 2小时29分52秒 |
| 按空闲价估算费用 | ¥30.63273668 | ¥14.14674332 |

AUTO 唯一失败属于 `dkim-verify`；CC 无失败。AUTO 经两组 fanout/merge/acceptance 后，因额外 ARC 边界测试进入两次修复和再验收；修复实际发现的问题不代表必然提高官方分数。原生 CC 可自行使用子 agent，这不是“多 agent 对单 agent”的严格对照。

费用按用户给定空闲价：每百万非缓存 input ¥1、cache read ¥0.02、output ¥4，合计 **¥44.77948**；reasoning 已包含在 output，不重复计价。这是根据用量推算，不是账单实扣。

原始材料已复制到 `G:\gb-bench\archives\mirrorcode-mailauth\pair-02\`，包含 `.eval`、console、agent archive、scorer 导出源码和逐例评分，逐文件 SHA-256 校验通过。manifest 同步保存为 `results/pair-02.json`。runner 与 runtime bundles 哈希仍匹配首轮归档；第一轮文件未覆盖。两臂第二轮容器均保留，未执行 cleanup。

## 7. 第三轮预算门槛：暂缓启动

截至台北时间 **2026-10-08 07:50:46**，第三轮 job/unit 尚未创建。余额检查记录见 [results/pair-03-budget-gate.json](results/pair-03-budget-gate.json)。

- 官方余额 API 正常：账户可用、CNY 余额 **¥104.84**。
- 已完成各对最高空闲价估算成本为第二轮的 ¥44.77948。只按空闲价，1.5 倍余量门槛向上取整为 **¥68**。
- 重新核实 [DeepSeek 官方定价](https://api-docs.deepseek.com/zh-cn/quick_start/pricing)：UTC+8 周一至周五（不含中国法定节假日）09:00–12:00、14:00–18:00 为高峰；其余时段及周末/中国法定节假日为空闲价。高峰价为每百万缓存读取 ¥0.04、非缓存输入 ¥2、输出 ¥8。
- 此时开始的长任务可能跨入高峰，不能沿用夜间全空闲的估算。因此按全程高峰价作保守预算（并非预测整轮都会在高峰）：`ceil(44.77948 × 2 × 1.5) = 135`。
- ¥104.84 低于该 **¥135** 启动门槛，按照预算规则不启动第三轮，停止自动接力、等待用户决定。这不是余额耗尽或执行失败，不给第三轮生成伪成绩。
- 用户先决定在台北时间 2026-10-08 18:05 的空闲时段重新查询余额，之后又取消了这个安排。DeepSeek 第三轮不再运行，mailauth 正式结果只有 pair-01 和 pair-02。

是否在 mailauth 上补跑 GPT 5.6 sol 的 200K 和 1M 对照，等 Ruff 出结果后再定。GPT 5.6 sol 上的 Ruff 实验是另一组独立实验，不并入本文 mailauth 结果。

## 8. 观察：CC 主会话上下文压力（假设，未证实）

两轮 CC 的模型窗口都是 1M（`modelUsage.contextWindow=1000000`），没有设置 `CLAUDE_CODE_AUTO_COMPACT_WINDOW`，使用默认 autocompact，两轮都没有发生 compaction。

| 指标 | pair-01 CC | pair-02 CC |
| --- | ---: | ---: |
| 主会话上下文峰值 | ≈976K / 1M | ≈592K / 1M |
| 子 agent 数 | 1 | 3（峰值约 310K / 512K / 663K） |
| 隐藏 | 684/702 | 702/702 |

pair-01 的大部分工作留在主会话里，上下文接近 1M 上限；pair-02 把更多工作交给子 agent，主会话保持在约 60%，结果也更好。一个合理的假设是：主会话接近上下文上限时质量会下降。目前每组只有一个样本，这不能证明因果关系。mailauth 也还没有把 CC 的主会话编排推到明显的上限，需要用更大的任务（例如 Ruff）观察这一点。
