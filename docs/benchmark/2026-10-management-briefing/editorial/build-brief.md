# B 编辑式 · 已批准的完整版

用户原话：「B · 编辑式好一点感觉」。门文件在 `../direction-approved.md`，现在是已选择方向后的执行，不要再做三方向或修改数据。

## 固定设计语法

先阅读 `../design-demos/reference/01-intent.html` 与 `02-results.html`，继承已选B的浅纸底、Inter/MiSans品牌字体、蓝色细线、主论点与证据分层。共同CSS在 `assets/editorial.css`，从 slides/*.html 使用 `../assets/editorial.css` 引入。资产将在 `assets/` 中，包括 gold-band.svg / claude-code.svg / kubernetes.svg、fonts.css 与两套本地字体。不要外部下载或依赖。每页1920×1080固定body，正文28–36px、注释至少20px、主要标题约86px，内容在y=900前结束；若用标准conclusion，它在924px，footer在底部35px。页面可有自己的CSS，不重复引入另一套导航/页码。父deck负责缩放和页码。所有页面末尾引入 `<script src="../assets/slide.js"></script>` 供iframe键盘事件桥接。

数据和结论唯一来源：`../README.md`。原报告 `../index.html` 是完整内容参考，可读相关section；但不要照抄它的深色样式。主页面要像会议演示，一页一个判断，不用成堆技术局限打断主线；必要证据可用脚注或点开详情。原实验量化边界不得改写。不要用WebSearch重新做事实研究或读取运行中的benchmark。

## 九页及责任

01-intent.html、05-marathon.html由主agent迁移已选两页。

新页：

- 02-benchmarks.html：先给出结论，再展示五个任务，由轻到重：DeepSWE、zstd-decoder、mailauth、Kubernetes、Ruff。stripe-clone 已按用户要求从演示移除。列表与详情左右分栏，使用同名原生details实现单项展开；详情区域绝对定位在右栏，不能推高画布。每项summary保留轻重标签、名称、任务描述与行数；点击后右侧展示具体任务和验收内容。行高由列表 Grid 均分，避免删除一项后残留空行。保留打印时closed的全景，不叠加详情。
- 03-deepswe.html：AUTO15/29 vs CC12/29，输出token10.7M/4.5M、平均耗时37/22min。2.4×是输出不是总量。zstd工作流与CC同为43/43，126M/64M、63/37min，可用页下方补充验证。结论中短任务质量略好持平，但token/time更高，Direct优先。
- 04-mailauth.html：两轮质量及总token/时间完整展示，不只挑第一轮。R1 AUTO694/702、407.9M、3h55m；CC684/702、336.6M、2h42m。R2 AUTO701/702、699.5M、8h09m；CC702/702、397.6M、2h30m。质量接近、有波动，成本更高，Direct优先。时间是Eval耗时，token包含缓存读取。
- 06-ruff.html：无闸门CC8.2%hidden、约309Mtoken、8h02m仍提前结束；必须加goal/完成约束。续跑对照还在进行，不声称AUTO已胜出。主要页面讲已知提前退出和继续执行问题，不堆排查日志。脚注或可点击详情如实保留AUTO首段窗口不匹配异常暂停、1.64%hidden，不让读者误以为AUTO无此问题。阶段截止2026.10.13，勿更新实时。
- 07-context.html：可见标注「架构判断」。按用户要求保留「单体应用中持续使用 goal，会不断触碰到主编排会话的上下文窗口限制，导致漂移。」精炼解释goal解决续跑条件，不扩大上下文；单主会话累积、触顶压缩、待办/验收约束漂移。属于机制分析，不能把Ruff无闸门提前退出归因于上下文触顶。视觉像编辑分析页，不用宏大技术流程图塞满。按用户要求删除该页底部“架构判断，非已证实因果……”整句，不添加替代小字；相关依据集中在 SOURCES.md。
- 08-auto.html：模型提出拆分，Runtime用程序而不是主模型会话持续编排；fanout让各Agent承载局部上下文，程序维护状态，合并后验收/返工。可用克制结构图搭配右侧解释，但延续B版排版不是深色UI卡片。不能写完全没有模型参与计划，也不能写程序必然保证业务验收通过。
- 09-recommendations.html：Direct=中短、目标集中、单Agent可覆盖；工作流=步骤可预定义、重复流程与固定质量关口；AUTO=目标明确路径动态、跨模块长任务、需要拆分整体验收。收束到「题目越长、越需要分工和整体验收，工作流与AUTO的优势越明显，质量更完整，token差距缩小甚至占优」。不再添加下一页结论，九页结束。

## 约束与检验

只创建分配给你的slides文件，不修改其他人的输出/原报告。不要写额外deck外壳，由主agent集成。真实logo保持原色，任务名是benchmark标识，不冒充第三方背书，不拿厂商品牌当研究项目logo。文字需完整落在1920×1080画布，横向不得越界。不要用隐藏overflow遮掉正文，容量不够要减少重复文字。无需本地启动服务器或浏览器，主agent会统一验证和导出。无新产品功能与开发计划变更。
