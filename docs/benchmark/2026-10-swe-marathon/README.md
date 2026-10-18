# SWE-Marathon：Gold Band vs Claude Code（2026-10）

## 1. 设置

- 基准：[SWE-Marathon](https://www.swe-marathon.org/)（abundant-ai/swe-marathon，Harbor 格式），本地 Harbor 0.20.0 + docker；agent 阶段网络为白名单，只放行模型端点。
- 模型：DeepSeek V4.1 Flash（`deepseek-flash[1M]`），effort max，两臂相同。
- Claude Code 臂：Harbor 内置 `claude-code` agent，版本 2.1.280，禁用 WebSearch / WebFetch。
- Gold Band 臂：headless `gold-band run`，Harbor 适配器见 `harness/`（评测机 `G:\gb-bench\adapter\gold_band_harbor_agent.py`）。按任务规模选配置：单 session 可完成的题用跳过 grill 的默认轻量工作流；单 session 做不完的题用 AUTO。
- 环境差异：Gold Band 臂额外安装 Node 22.14（无 zstd 能力）供 claude-agent-acp 使用；镜像无 git 时上传 git 2.43 并提交 `/app` 初始状态。

## 2. 正式成绩

| 题目 | 臂 | 版本 | reward | partial | 通过 | agent 耗时 | 备注 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| zstd-decoder（5h） | Gold Band 轻量工作流 | `--workflow` CLI | 1 | 1.000 | 43/43 | 63 分钟 | token 约 1.26 亿 |
| zstd-decoder（5h） | Claude Code | 2.1.280 | 1 | 1.000 | 43/43 | 37 分钟 | token 约 6400 万 |
| kubernetes-rust-rewrite（10h） | Gold Band AUTO | 无"结束前验收"规则 | 0 | 0.9993 | 1406/1407 | 215 分钟 | 追改 `Cargo.toml` 违规后的成绩：原始交付修改了受保护的 `Cargo.toml`，评测恢复后编译失败（原始 reward 0、partial 0）；事后仅把 all-in-one 对 axum 的直接依赖改为调用 api-server 暴露的 `serve`（2 行），再按官方 verifier 评测 |
| kubernetes-rust-rewrite（10h） | Gold Band AUTO | 加"整个需求开发完成后统一验收"规则 | **1** | 1.000 | 1407/1407 | 165 分钟 | 原始交付直接满分，无人工干预；token 约 1.73 亿；11 个动态节点（wave2 fanout 含 merge 与组验收），最后由 `final-requirement-acceptance` 统一验收通过后结束；榜单 75 次运行无一满分（09:22 的旧措辞运行已中止，不计） |
| kubernetes-rust-rewrite（10h） | Claude Code | 2.1.280 | 0 | 0.9986 | 1405/1407 | 97 分钟 | 原始交付即可编译；失败为 Service `targetPort` 默认值、ReplicaSet 强删 Pod 后自愈；Harbor 统计 input 3.24 亿（其中 cache 3.21 亿）、output 184 万；与 AUTO 并行运行 |

| stripe-clone（4h） | Gold Band AUTO | 加"整个需求开发完成后统一验收"规则 | 0 | 0.917 | 11/12 gate（pytest 82/83） | 66 分钟 | 11 个功能 gate 全过，唯一失败为 anti_cheat 源码扫描：agent 自写的 `/app/tests/*.py` 含 `import stripe`，3DS `next_action` 中含 `hooks.stripe.com` URL；该扫描规则未写入题面。token 约 5210 万。榜单 80 次（2026-05-06 至 07-08）无满分、partial 最高 0.333，但均使用旧版题面（约 4.8K 字符的概述）；我们使用 v1.1 题面（2026-07-31，约 9.4K 字符，新增测试卡、状态机、幂等、webhook、订阅等精确规格），与榜单不可直接比较，需同版本 Claude Code 对照 |

## 3. 观察

- zstd-decoder 两臂均满分，对该模型无区分度，只作冒烟。两臂都使用了镜像中残留的 `libzstd.so.1` 做测试参照（CC 用于整个开发过程，Gold Band 仅 accept 阶段），交付物均通过防作弊检查。
- kubernetes-rust-rewrite 第一次 AUTO：11 个 single 节点按 crate 依赖顺序推进，未使用 fanout，因而没有 group acceptance；最后的开发节点直接结束。表中成绩为追改 `Cargo.toml` 违规后的结果：受保护集成测试 1406/1407 通过，唯一失败为 Service `targetPort` 默认值；该追改发生在时限之后、由人工完成，对外引用时须同时注明原始交付因违规编译失败。榜单上所有配置 binary 均为 0；partial 最高为 Grok Build + Grok 4.5（5 次均约 0.999），其次 Claude Code + GLM 5.2 平均 0.54（含 2 次 0）、Claude Code + Opus 4.8 0.42。1407 条计分测试中 1390 条在 `/app` 中可见、仅 17 条隐藏，partial 接近满分不难，区分度在最后几条行为测试和 binary 通过上。
- 由此修复：AI-DYNAMIC 路由规则改为 `end` 只用于已通过验收的交付（见 MVP 计划 2026-10-06 条目）。
- 加规则后的 kubernetes AUTO：按 foundation → api-server/controller-manager/kubelet/kubectl/small-services 的 fanout 并行开发，合并后做 all-in-one 集成，最后整体验收一次（未按阶段逐个验收），受保护集成测试 1407/1407 全过，reward 1。同模型 Claude Code 97 分钟 1405/1407（reward 0）。
- stripe-clone AUTO：bootstrap → core-slice → fanout 两个分支（资金/账单、webhook/平台）→ merge → 组验收发现 BLOCKER（SDK 的 `create(confirm=true)` 一次调用未生效）→ 派修复节点 → 再验收通过后结束，验收失败派修复的闭环按预期运行。功能 gate 全过，但 anti_cheat 扫描 `/app` 下所有文件，agent 自己的 SDK 测试与 3DS 跳转 URL 命中禁止规则，reward 0。

## 4. 已知风险

- 榜单数据（2026-05 至 07-08）早于任务 v1.1（2026-07-31）。stripe-clone 题面在 v1.1 中扩写近一倍，榜单成绩不可作为基线；kubernetes-rust-rewrite 题面仅新增时限说明，但隐藏黑盒测试在 2026-06-12 才加入，部分榜单运行早于该版本。对外对比以同版本、同模型的 Claude Code 对照为准。
- 榜单 stripe-clone 轨迹中仅 2/65 次可见向 `/app` 写入会触发 anti_cheat 的内容，其低分主要来自功能 gate 失败，而非 anti_cheat。
- AUTO 使用 fanout 时，分支 worktree 位于 `/app/.gold-band/worktrees/`，超时被终止时未合并分支不计分；正式成绩严格按任务时限计算。
- kubernetes 两臂并行运行，共享 WSL 24 GB 内存（每容器上限 16 GB），存在内存竞争。
