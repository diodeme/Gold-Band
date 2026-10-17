# 三个视觉方向 · 已选择 B 编辑式

[打开六页对比概览](index.html?ov=grid)

这里保留三种方向的两页样稿，展示相同的「测试意图」与「Kubernetes 重量级结果」。用户已选择 B 编辑式，选择原话记录在 [direction-approved.md](../direction-approved.md)。[完整九页 HTML](../editorial/index.html#1) 与 [PDF](../editorial/Gold-Band-Benchmark.pdf) 已在 editorial 目录交付。原始九页 `../index.html` 保留不变。

| 方向 | 查看两页 | 设计方法 | Form 的内容来源 |
| --- | --- | --- | --- |
| A · 图解式 | [roulette.html](roulette.html?ov=grid) | 秒数轮盘 31 % 20 + 1 = 12，PPT 图谱箭头企业版 | 单体与编排路径分岔，最后汇合到共同验收标准。 |
| B · 编辑式 | [reference.html](reference.html?ov=grid) | 迁移 Stripe 年度信的论点与证据分层方法，不借用其品牌 | 主问题与判断依据分栏；完整通过的数字与事实对照表并列。 |
| C · 舞台式 | [studio.html](studio.html?ov=grid) | 受 Pentagram 信息设计与字体构图方法启发，不声称参与或背书 | 以完整交付为共同终点，用 1,407 与最后两项缺口组织结果页。 |

## 观看

- 双击 `index.html`，概览中点击任意一页放大。
- `←` / `→` / Space 翻页，`Esc` 回概览；也可点击左右边缘翻页。
- 以上链接指定网格概览；模板也保留图库模式，使用预生成 PNG，避免大量重复 iframe。
- 每页原画布 1920×1080，由原模板 JS 缩放和留黑边。请保留整个目录：页面、logo、字体与截图均为本地资产。
- 不依赖网络，不需要安装 npm 包或启动服务器。

## 截图

- [六页总览](previews/overview.png)
- A：[意图页](previews/roulette-intent.png) / [结果页](previews/roulette-results.png)
- B：[意图页](previews/reference-intent.png) / [结果页](previews/reference-results.png)
- C：[意图页](previews/studio-intent.png) / [结果页](previews/studio-results.png)

这些截图均由实际 Chromium 渲染，不是概念插图。

## 来源与实现

- 共用输入：[design-spec.md](design-spec.md)、[brand-spec.md](brand-spec.md)、[product-facts.md](product-facts.md)。
- 品牌与字体：[assets/README.md](assets/README.md)。真实 Gold Band 标志来自项目 `web/public/logo.svg`，不再使用原报告的几何金环。
- B 方向现实参照：[Stripe 2024 Annual Letter](https://stripe.com/annual-updates/2024)。只迁移版面层级，不引用其业务指标。
- 四个 deck 外壳从已安装的 `huashu-design/assets/deck_index.html` 复制，只配置标题、manifest、viewport 与对比概览列数上限；不另写概览 / 缩放框架。
- `prepare-assets.mjs` 可准备素材；`build-indexes.mjs <模板绝对路径>` 可生成初始外壳。生成器使用排他创建，不覆盖已有人工改动文件。

## 验证记录 · 2026-10-14

- 六页原生尺寸均为 1920×1080，所有本地字体加载成功，无破图、无文字越出画布、无小于 18px 的正文节点。
- 肉眼检查了全部六张真实截图；修正 A、B 结果页时间列为 Agent 耗时，避免与端到端 Eval 时长混淆。
- 概览点击、方向键翻页、Esc 返回通过，页面错误列表为空。
- 原模板按窗口宽度排六页时，在 1440px 得到四列，在 1920px 得到五列，不满足三方向上下配对。新增列数契约测试先复现失败，再以生成器的列数上限配置修正；未重写布局逻辑。
- `node --test docs/benchmark/2026-10-management-briefing/design-demos/previews.test.mjs docs/benchmark/2026-10-management-briefing/presentation.test.mjs`：**22/22 通过**，包含原报告十项测试。
- 当前阶段未导出 PDF，未生成完整新版九页，未提交 Git，未动运行中的 benchmark。

## 自评审

- 过度设计：本阶段六张静态页，共用已存在品牌资产、字体与模板。没有新增框架、产品状态模型或远端服务。方向批准前不批量重做九页。
- 性能：固定最多六张概览页、字体本地共享；图库用生成的截图。无日志读取、轮询或实时数据聚合。模板的页面位置记忆仅用于演示，不影响产品数据。
