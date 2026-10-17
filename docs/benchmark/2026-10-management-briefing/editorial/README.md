# Gold Band Benchmark · B 编辑式完整版

- [从第一页开始演示](index.html#1)
- [九页概览](index.html?ov=grid)
- [PDF 备份](Gold-Band-Benchmark.pdf)
- [数据口径与来源](SOURCES.md)

用户已选择 B 编辑式；共九页。材料截止 2026-10-13（台北时间），不是实时看板。HTML 与 PDF 的主页面内容一致。

## 会议使用

解压整个离线包，用 Edge / Chrome 打开 `index.html`，无需服务器或网络。

- 点击概览中的页面进入演示。
- `←` / `→`、PageUp / PageDown、空格：翻页。
- `Home` / `End`：首页 / 末页；数字 1–9：直接跳页。
- `F` 或右上“全屏”：进入 / 退出全屏。
- `Esc`：先收起当前详情；没有展开内容时返回概览。
- 第 2 页点击 benchmark 查看具体题目；支持键盘 Tab、Enter / Space。
- 第 6 页可展开首段记录与测试口径。
- 若只需发送给参会者或现场备用，直接使用 PDF。PDF 固定九页，不包含交互展开内容，相关口径见 `SOURCES.md`。

**请勿只复制 index.html。** 这是多文件离线 deck，需要 `assets/`、`slides/` 和 `previews/`。使用 ZIP 搬运最稳妥。

## 九页顺序

1. 测试意图：什么时候值得让多个 Agent 协作；Claude Code 明确标注允许分发 subagent。
2. 结论先行：中短任务用 Direct，长任务用编排；五种 benchmark 附参考实现行数，可点击展开。
3. DeepSWE 与 zstd：质量略好或持平，额外开销更高。
4. mailauth：与 Claude Code 质量接近，AUTO 的 token 与耗时更高。
5. Kubernetes：完整验收，记录 token 约少 47%，时间仍更长。
6. Ruff：Claude Code 未完成就主动退出；AUTO 通过编排持续推进，无需额外提交闸门。基础设施中断与恢复记录保留在展开证据中。
7. 上下文与漂移：长任务的架构判断。
8. AUTO：模型提出拆分，程序承担状态与持续调度。
9. Direct / 工作流 / AUTO 使用建议。

## 交付与验证

- 已使用真实本地 Logo、Inter / MiSans 字体；许可声明位于 `assets/LICENSES.json`。
- 复用 huashu-design 多文件 deck 模板的概览、缩放与翻页，不新增产品页面或应用依赖。
- iframe 内的原生详情保留焦点；导航消息只接受当前活动页面，避免无关 iframe 改变页面。
- 1920×1080 下九页文字无画布越界；PDF 恰好九张横向页面。实际 PDF 已用 Chromium PDF 阅读器抽样查看，中文与数字正常。
- 五个 benchmark 使用同组互斥展开；stripe-clone 不纳入本次演示。第 7 页删除底部补充说明，保留主结论，口径集中在 SOURCES.md。
- 真实浏览器测试通过：点击、Space、Enter、Escape、跨 iframe 翻页、全屏、无效 hash 边界、刷新位置、概览跳转、1280×720 缩放；页面错误为零。
- 完整版与原版回归测试共 **29 项通过**。本轮先验证旧稿仍含第六项任务与第 7 页补充页脚，再确认删除后通过。真实浏览器验证了五项详情展开、结论保留且无补充页脚、桌面及笔记本尺寸的标题与交互；原版与方向样稿保留在上级目录。
- 无 Rust 测试、产品启动或用户 Gold Band 配置写入；未启动、停止或续跑 benchmark。

## 维护工具（仅开发者使用）

演示不需要 `tools/`。工具依赖隔离安装于 `tools/node_modules/`，不进入离线 ZIP。

```text
node --test tools/presentation.test.mjs
node tools/test-browser.mjs <Chrome可执行文件绝对路径>
node tools/export-pdf.mjs --slides slides --out Gold-Band-Benchmark.pdf --executable <Chrome可执行文件绝对路径> --screenshots previews
```

PDF 导出器改编自 skill 脚本，使用真实 Windows file URL、等待本地字体 / 图片完成、逐页验证一页一张后合并。`Gold-Band-Benchmark.verification.json` 记录生成时的九页检查结果。

性能与过度设计评审：固定九页，本地共享字体，图库使用预生成截图；无在线数据加载、轮询、无界缓存或新增产品状态。导出所需依赖只属于独立文档工具。
