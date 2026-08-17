# 会话式主页

## 信息架构

会话式主页是 Gold Band 的新主入口，以 chat bot 为核心心智，替代传统工作台的任务编排心智。

### 页面结构
- **左侧栏**：会话列表、快捷入口、置顶区、工作空间分组
- **右侧主区域**：新建会话输入框（home）或当前 session 对话（runtime）
- **右侧工作区**：文件变更、文件/资产查看、源码管理和其他可持续操作的 Tab。区块与资源类型图标属于静态功能标识，统一使用 `foreground` 随主题反色；新增入口、文件标题和源码管理标题不得使用 `primary` 充当通用图标色。增删行、冲突、运行中和选中态继续使用对应语义色。

### 视觉层级与信息分组

- 会话页使用四档语义排版：workspace 标题是侧边栏分组锚点，使用 UI 基准字号和 600 字重；会话标题与消息正文使用 UI 基准字号，会话标题使用 500 字重，正文使用 400，不放大成页面大标题；普通会话列表项使用略小一档的紧凑字号和 400 字重，当前选中项使用 500；session、run ID、相对时间、用量与计时使用约 10–12px、400 字重和更低前景对比。层级必须同时由字号、字重和对比关系表达，禁止让会话标题压过正文，也禁止让 workspace 标题弱于其下的会话项。
- 颜色层级固定为标题 `foreground`、正文 `foreground/90`、辅助标签 `muted-foreground`、ID/时间等元信息 `muted-foreground/55~75`；选中态可恢复完整可读前景，但不得用 primary 色替代文字层级。
- 间距遵循“同组紧、跨组松”：标签与值保持 4–8px，组件内部保持 8–12px，相邻信息组保持约 16px，不同消息保持约 20px。空白必须用于标记信息边界，不以大块无内容区域代替分组。
- 会话顶栏、消息流、底部运行统计与 composer 使用同一 20px 水平主轴；标题与 session 切换属于同一顶栏组，统计与 composer 属于同一底部操作组。调整只影响排版，不改变信息数量、入口、选中状态或会话生命周期。
- 消息流、底部运行统计与 composer 共用居中的阅读轨道，最大宽度为 56rem；可用宽度不足时退化为全宽并保留 20px 安全边距。该宽度在默认 14px 中文正文下约对应 50–60 个汉字的有效行长。会话顶栏仍保持桌面工具栏心智，不强制进入正文轨道。

### 入口路径
- `/chat` — 会话主页（新会话输入框）
- `/chat/projects/:projectId/tasks/:taskId/runs/:runId` — 会话运行时
- `/chat/agents` — Agent 管理
- `/chat/contexts` — 上下文管理
- `/chat/settings` — 设置

### 默认入口与工作台观察期
- 桌面根路径 `/` 与显式 `/chat` 都进入会话主页，会话主页是当前唯一产品主入口
- 共享自定义顶栏不展示 Workbench / Conversation toggle，侧边栏也不提供工作台入口
- 不读取历史 `gold-band-ui-mode` 偏好覆盖启动入口，避免曾使用工作台的用户继续默认落入旧形态
- 旧工作台页面与 `/tasks` 等显式 deep link 暂时保留，观察期结束后再决定是否删除，不增加兼容层或新的隐藏切换机制

## 新建会话流程

### 时段欢迎标题
- 新会话主页不展示系统状态式的“新会话”大标题，改为依据桌面系统本地时区显示邀请式欢迎语：05:00–08:59“早上好，今天想一起做点什么？”；09:00–11:29“上午好，今天想一起做点什么？”；11:30–13:59“中午好，今天想一起做点什么？”；14:00–18:29“下午好，今天想一起做点什么？”；18:30–23:29“晚上好，今天想一起做点什么？”；23:30–04:59“夜深了，想一起做点什么？”。
- 欢迎语是页面的视觉锚点，统一使用主题 `title` 语义色，不得通过 `foreground` 透明度弱化。浅色主题必须呈现接近墨黑的标题，与石墨正文、中灰辅助文字和浅灰边界形成明确层级，避免页面灰蒙蒙一片。
- 浅色主页主内容面固定使用纯白背景；灰色只用于左侧栏、选中面、用户消息和少量输入控件。不得通过半透明 card/background 在主内容中铺出新的大面积灰阶。
- 英文欢迎语使用更短的自然表达，不强行区分英语中不存在的“早上 / 上午”和“中午 / 下午”问候：早上与上午统一为 `Good morning. What shall we work on?`，中午与下午统一为 `Good afternoon. What shall we work on?`，晚上为 `Good evening. What shall we work on?`，夜深为 `It's late. What shall we work on?`。
- 欢迎标题统一消费 `title` 语义 token；浅色主题使用接近墨黑的标题建立视觉锚点，深色主题由各自色板提供高对比标题色。不得在组件中通过前景色透明度削弱标题，也不得为不同主题硬编码颜色。
- 欢迎标题的时间状态由独立组件管理，首次进入时计算一次，并仅为下一个时段边界安排一个单次定时器；禁止使用按秒或按分钟轮询。
- 应用从休眠或后台恢复、页面重新可见、窗口重新获得焦点时，必须按当前系统本地时间重新校准，以覆盖 WebView 后台定时器暂停和系统时区变化。
- 跨时段只替换标题文本节点，不刷新页面，不重置 composer 正文、附件、workspace、Agent、模型、权限或运行模式状态。
- 侧边栏展开或收起时，欢迎标题与 composer 始终按右侧可用主区做横向几何居中，不根据顶栏品牌或 composer 内部控件重量做水平偏移。纵向使用光学居中：通过 64–80px 响应式底部布局留白让整组内容自然上移约 32–40px，抵消标题下方多块配置区带来的下坠感；禁止使用不参与布局的 transform 位移。

### 输入区域

- 会话壳、侧栏、阅读区与 prompt-kit composer 通过稳定主题角色消费主题包 recipe；不得在会话业务组件中根据 `themeId` 分支。
- `data-theme-role` 只标记真正拥有对应视觉 surface 的元素，不能因为内容逻辑上位于 composer 内就把状态行再次标为 `composer`。ACP usage/processing 行继承外层 composer，不单独产生背景、边框、圆角或 elevation；普通 activity 保持无边框，tool 审计行只保留既有下分隔线，普通 tool card 才保留完整卡片边界。主题 recipe 是 role 的组件层默认值，不能抹掉 prompt-kit/shadcn 显式变体已有的 focus ring、透明背景、局部阴影、单边分隔或 transition。
- 会话标题中的 session 切换列表使用统一 `popover` 主题角色；当前 Gold Band 与技术中性均由各自包提供实底 popover，业务组件不得写固定透明度。
- 主题、明暗或视觉质量切换只触发 CSS 样式重算，不读取会话数据、不重播 Markdown、不改变 composer 草稿、会话选中态或右侧工作区资源身份。
1. 文本输入框：用户输入任意需求文本
   - 主页 composer 与会话追问统一复用 prompt-kit 自动尺寸输入能力，不维护独立的原生 textarea 高度逻辑，也不开放浏览器原生手工拖拽调高角标；两处都只随内容增长到 320px 上限，超过后转为输入区内部滚动。
   - prompt-kit textarea 本体在所有主题下保持透明并继承 composer 外层表面色；主页发起会话与会话追问不得出现独立的内层灰色输入色块。
   - 主页主内容使用较紧凑的 `max-w-3xl` 可读宽度；正文输入区初始最小高度为 56px，随文字换行和内容增加同步增高，最高增长到 320px。未达到上限时隐藏正文区滚动条；超过上限后固定正文区高度并仅在正文区内部滚动，顶部工作空间信息区、底部提示、附件、模型、权限和发送操作不随正文滚动。
2. 工作空间信息与工作位置：普通快速对话的发起 composer 在输入面上方显示一条 32px 低矮信息栏；该区域只属于普通发送模式，不进入会话详情 composer，也不在定时任务创建模式重复展示。信息栏主体左右各内收 48px，宽度始终小于输入框；顶部两端使用 16px 圆角，背景在明暗主题中统一消费比页面底色更高一层的 `surface-high` 语义材质，使轮廓始终可辨，不得回退到 `conversation-background` 或用单一 `muted` 透明度造成层级消失、反转。信息栏与输入面在正常文档流中紧邻但不重叠；栏底左右各使用一个 16px、与信息栏同色的反向四分之一圆连接肩，透明圆心位于外侧上角，使轮廓向信息栏内部弯入，再平滑外展并落在输入框左右各 32px 处；顶部圆角与底部连接肩分别占据栏高的一半，在中线之后衔接，禁止让连接肩穿过顶部圆角透明区形成竖缝。顶部主体与曲线底部必须共同收窄，不得只收窄顶部直边、让底部继续延伸到输入框上圆角切点，也不得使用向外鼓出的实心圆角。连接肩不得伸入半透明 composer；输入框保留完整上圆角，半透明区域下方只允许存在会话背景，避免壁纸场景透出横向色带。禁止使用负 margin、z-index 叠放、阴影扩散或重叠透明填充。信息栏在窄宽度下继续以输入框宽度为上限，内部名称允许截断，不得造成横向滚动或遮挡欢迎标题。
   - 信息区左侧复用现有工作空间选择器，工作空间入口从普通快速对话 composer 底栏移入此处；控件按图标、当前名称、箭头和内边距的实际内容宽度收缩，超长名称只在真实截断时通过 shadcn/Radix Tooltip 展示完整名称，不使用原生 `title`。工作空间与工作位置两个触发器静态时都必须透明并继承信息栏背景，只在 hover、键盘 focus 或菜单展开时共享主题 `accent / accent-foreground` 交互态；菜单关闭且指针离开后必须恢复透明，不显示常驻选中色块。定时任务底栏的胶囊变体继续保留自身背景。
   - 工作空间和工作位置菜单关闭后的焦点归还都按输入方式处理：鼠标或触控选择、再次点击触发器或点击外部关闭后，不把焦点重新放回触发器，也不保留误导性的选中描边；键盘选择或关闭后仍由 Radix 将焦点归还触发器并显示 `focus-visible`，保证后续键盘导航连续。禁止通过删除共享 focus ring 或强制 blur 所有输入方式掩盖该差异。
   - 普通快速对话当前选择的工作空间属于应用运行期的 draft 上下文：用户切换后，无论进入设置还是查看其他工作空间的会话详情，再点击全局“快速对话”都必须恢复该 draft。只有 draft 尚不存在时，才先使用当前会话所属工作空间，非会话页再回退最近会话工作空间。只有在某个工作空间下显式点击“新会话”，才将 draft 切换为该工作空间。draft 选择不得写入或重排 `lastConversationWorkspace`，左侧工作空间置顶仍只由最近实际创建、重跑或进入的会话驱动。
   - 同一区域提供工作位置选择器，仅包含“主工作区”和“新工作树 / New worktree”。“新工作树”使用 Git fork 图标，悬浮或键盘聚焦提示“创建副本，以便并行工作 / Create a copy for parallel work”。工作位置触发器与工作空间触发器统一使用 28px 高度、相同圆角、水平内边距、图标和箭头规格，宽度仅随各自内容自然收缩；不得让 shadcn `SelectTrigger` 的默认 size variant 把工作空间控件撑高。工作位置偏好按 `projectId` 记忆，切换工作空间后恢复该工作空间最后一次选择，不使用跨工作空间的单一全局值。
   - 非 Git 工作空间仍展示“新工作树”；用户选择时先复用 AI-DYNAMIC 的 Git capability preflight，并通过既有 `GitRequirementDialog` 展示结构化错误与恢复动作。校验失败不得保存 worktree 偏好、创建 task/run 或静默回退主工作区。
   - 定时任务创建模式继续在 composer 底栏展示工作空间选择器，以保持其既有创建上下文；定时任务固定使用主工作区，不展示或继承快速对话的工作位置选择。
   - composer 底栏必须按自身可用宽度而不是整窗 viewport 响应。普通快速对话底栏只承载附件、配置与发送，并与会话详情 command bar 共用紧凑尺寸基线：附件入口为 28px，模型、权限和发送为 32px；正文与底栏之间只使用留白，不绘制额外顶部分割线。WORKFLOW / AUTO 没有内联模型与权限控件时，附件与发送 split-button 在紧凑的 `xs` composer 容器档位即直接同排。Direct 保留配置型布局：最窄容器下附件独立占据操作起点，模型、权限按可用宽度换行；中等宽度中模型与权限等宽并列、发送位于下一行末列；达到桌面宽度后再恢复完整单行。模型与权限等配置选择器必须允许布局层覆盖为 `w-full / max-w-none`，禁止组件内部最小宽度决定偶然换行结果。
   - 桌面单行工具栏中，附件入口保持固有宽度；模型、权限和发送所在配置列使用剩余空间，内部固定使用 `模型 minmax(0,1fr) / 权限 minmax(0,1fr) / 发送 auto` 三列，模型与权限保持等宽，发送末列的右边缘必须与工具栏内容区右边缘对齐。
   - 处理模式与 Direct Agent 选择区使用同一 composer 容器断点：最窄容器中标签置于控件上方，处理模式三项等宽，Agent 列表始终保持单行；内容超出可用宽度时保留横向滚动能力，但不显示可见滚动条，并必须显式禁止纵向溢出。鼠标滚轮、触控板和键盘焦点导航不得因隐藏滚动条而失效。Agent 行总高保持紧凑的 56px：外层只保留 4px 上下 padding，滚动容器内部再保留 4px 安全空间，避免 40px 选中药丸及其焦点边缘被裁切。宽度足够后再恢复标签与控件同排。处理模式、Agent 等选择控件继续保持不低于 36px 的触控目标；发送 split-button 在各宽度下统一使用 32px 操作基线和内容宽度，独立换行时右对齐。
   - 发送与定时创建属于同一个提交模式状态机，两种模式复用同背景的紧凑 split-button 和同一正文提交资格。正文为空时，发送与“创建定时任务”主按钮均使用 shadcn Button 原生禁用态；ChevronDown 与定时配置按钮不随正文禁用。ChevronDown 仅承担模式切换，24px 箭头区与主操作之间不显示分割线；定时配置通过右侧工作区 Tab 编辑，不以模态遮罩打断 composer 与工作空间上下文。管理页通过 `/chat/scheduled-tasks/new` 进入主页时，Composer 必须消费该类型化导航意图并直接呈现定时创建态；普通 `/chat` 仍初始化为会话模式。
   - 主导航选中态按当前承载页面归属计算：`conversation-home` 与 `scheduled-task-create` 都使用会话主页 Composer，因此统一选中“快速对话”；`scheduled-tasks` 与 `scheduled-task-detail` 才选中“定时任务”。创建入口来源不得覆盖目标页面的导航归属。
   - 2026-08-11 实现验收：从管理页点击“创建定时任务”后进入 `/chat/scheduled-tasks/new`，Composer 自动呈现定时创建态并打开右侧“定时任务设置”Tab，同时侧栏从“定时任务”切换为选中“快速对话”；通过 ChevronDown 切回“发送”后返回 `/chat`。直接刷新 `/chat` 仍初始化为普通发送态，且右侧定时配置不残留。
   - 左侧栏的手动展开/折叠意图与窗口宽度导致的临时自动折叠分开管理。窄屏不得改写或持久化手动折叠值；再次拉宽时自动恢复，右侧工作区与左侧栏同时折叠时固定先恢复右侧、再恢复左侧。
3. 处理模式选择：WORKFLOW / AUTO 切换
4. AUTO 模式：
   - 固定 Agent 策略下显示 agent、模型、权限模式下拉；agent 可以覆盖 AUTO tab 当前配置，模型可为空
   - 动态 Agent 策略下显示 Dynamic Agent 标识和权限模式下拉
   - 显示非必填全局 Goal 输入框
   - 全局 Goal 输入框必须保留用户原始输入，包括词间空格、连续空格、开头空格和输入末尾的临时空格；创建会话 payload 边界只把纯空白输入规范化为未设置，不裁剪非空文本
   - 提供跳转 AUTO tab 的快速入口
5. WORKFLOW 模式：显示工作流模板下拉，并提供跳转工作流 tab 的快速入口；内置“默认完整工作流”展示需求采访开关，内置“默认轻量工作流”展示需求拷问开关，自定义模板不展示可选入口开关

### 创建规则
- 标题 = 输入内容首行前 N 个 Unicode 字符；N 由项目级 `configs/app-config.toml` 的 `conversationAutoTitleMaxChars` 控制，默认 12
- 无文字时使用 i18n "New Task / 新任务"
- 自动标题仅在首次创建时生成
- 调整 `conversationAutoTitleMaxChars` 只影响之后新建会话的自动标题，不回写历史 task，也不影响用户手动重命名
- 描述为空，内容为完整输入

### 工作树创建与运行目录

- `RunState.worktree { path, branch, fork_commit }` 是会话运行工作位置的 canonical fact；缺少该字段表示主工作区。会话 metadata 中的 `work_location` 只保存创建与重跑意图，详情页和执行路径不得从偏好反推当前 run 的事实。
- 选择 worktree 后，run 先进入既有 `PreparingWorkspace` 阶段，在 Gold Band 用户数据目录的受管 `worktrees/` 下创建独立目录，并以源仓库当前 `HEAD` 创建专用分支。路径和分支使用 run identity 的确定性短哈希生成，不把 worktree 写入源仓库，也不使用名称反查身份。
- Agent 的 `workspace_dir` 使用该 run worktree；adapter 的 `adapter_workspace_dir` 继续指向原项目工作空间，以保持配置和能力发现边界。AI-DYNAMIC 的 main workspace 使用外层 run 的实际工作目录，因此可以在会话 worktree 内继续按现有 fanout 机制创建子 worktree。
- 创建工作树沿用 AI-DYNAMIC 已有的 Git helper、结构化错误、创建清理和停止 UI。Git 命令本身不强制中断；若用户在创建期间停止，外层 run 先持久化 Paused，创建完成后重新读取 durable run，禁止启动 Agent，并保留已创建工作树供检查。重跑 worktree 会话时从当时新的 `HEAD` 创建新的 run worktree，不复用旧 run 目录。

### 附件上传

新建会话支持用户附带文件作为 task 初始输入：

- **入口**：纸夹按钮选择文件、拖拽文件到 composer、粘贴图片/文件；桌面端必须在基础 Tauri 配置和 channel overlay 中关闭原生 WebView file-drop，让文件拖拽进入前端 HTML5 drop zone，拖入 composer 时稳定显示可投放状态
- **统一功能区**：快速对话与会话详情复用 composer 内部的上下文功能区；该区域与输入区共用同一背景、外边框和 focus ring，没有独立附件面板。无附件时不占高度，宽度不足自动换行，最多两行后内部滚动。
- **预览**：图片附件只显示固定方形缩略图，不展示文件名，hover/focus Tooltip 显示名称与大小，点击进入右侧工作区图片预览；非图片文件显示图标和截断文件名并沿用文本预览。
- **操作**：每项支持删除；图片删除按钮在 hover/focus 显示，触摸设备常显。旧的 composer 外部附件区域和工具栏文件数量标签删除，不保留双入口。
- **校验**：最多 10 个文件，单文件 ≤ 25 MB，总大小 ≤ 100 MB；支持常见文本/代码/文档/图片类型

附件生命周期：

- 初始附件绑定 task，保存在 `authoring/attachments/`
- 重跑复用 task 级初始附件，无需重新选择
- 继续对话新增附件走 ACP 输入内容，不混入 agent 输出附件目录

### 发送前校验
- 校验 workspace、运行模式、agent、模型、AUTO 设置、workflow 模板有效性
- 创建新会话时必须要求发起用的 workflow 合法；WORKFLOW 模式校验指定模板，AUTO 模式校验运行时生成的 `AI-DYNAMIC -> end` 工作流
- 校验附件路径、类型、大小、数量
- 通过：创建 task + run，进入该 run 的当前 session
- 失败：展示缺失项和恢复路径
- WORKFLOW 模式发起前必须复用运行模式管理页的工作流模板合法性校验；非法模板不创建 task/run，不清空输入，错误展示在 composer 下方
- AUTO 配置和 WORKFLOW 模板阻断错误展示在 composer 下方，保留修复按钮跳转运行模式管理；Direct 校验错误同样内联展示，但不提供“去配置 / 修复”跳转，因为 Direct 的全部会话配置都在当前 composer 内完成。错误不自动消失，直到用户重新发送或页面重新加载

### 草稿保留
- composer 的未发送正文与已选附件作为 App 层共享草稿统一管理，存活期独立于 composer 组件挂载；从主页跳转运行模式管理（去配置 / 修改 AUTO / 修改工作流）、打开其他会话窗口、进入设置等普通离开会话主页后再返回时，正文与附件原样保留
- 草稿状态由会话 composer 局部 boundary 拥有并通过 context 下发，App 只保留 `reset` 窄接口用于创建成功或切换目标工作空间；输入每个字符不得触发 App 大树重新 render。
- 附件草稿中的浏览器图片预览 URL 由 App 层草稿 owner 统一管理，普通页面离开不释放；仅在显式移除/清空附件、创建会话成功、切换目标工作空间或应用卸载时释放
- 草稿仅在创建会话成功或切换目标工作空间时清空，普通跨页面导航不清空
- 右侧工作区的 Tab、活动 Tab、展开状态和显式打开 revision 统一归属于草稿或完整会话 identity；任一会话打开或关闭右栏不得影响其他会话。右栏宽度属于用户布局偏好，继续跨会话复用。
- 快速会话创建成功时，右侧工作区只把该草稿作用域下的上述完整状态和资源 locator 一次性移交给本次新建出的目标会话；资源的 `scopeKey` 及依赖作用域生成的 key 必须同步重绑定，随后删除草稿条目。其他既有或后续会话不得读取或复制这次继承的状态。
- 创建 task + run 属于阻塞型本地文件/运行时操作，桌面端命令必须放入后台 blocking task，前端发送按钮进入 busy 状态，避免快速会话创建期间整窗卡死或重复提交

### 运行模式持久化
- 快速会话复用上一次会话的运行模式选择
- 运行模式状态以 workspace 为一级作用域，不使用跨 workspace 共享的单一前端快照；切换草稿目标时先展示该 workspace 的内存快照，并重新读取其持久化配置。
- Direct 在 workspace 内再以 `agentType` 为二级作用域保存模型和权限。选择 Agent、模型或权限时，保存请求必须显式携带当前 composer 的 `projectId`，不能依赖切换中的默认 workspace 闭包。
- 同一 workspace 的运行模式写入必须按用户操作顺序串行提交；切回 workspace 重新加载前必须等待该 workspace 已排队的写入完成，避免较早的 Agent 默认配置晚到并覆盖后续模型/权限选择。不同 workspace 的写入可以并行，互不阻塞。
- 如果上一次选择 AUTO，则复用 composer 中的会话级 AUTO 配置，包括固定策略下的 agent / 模型、权限模式和全局 Goal
- 会话级 AUTO 全局 Goal 的持久化编辑态不在每次按键时 trim，避免受控输入框反灌时吞掉空格；创建会话时也不裁剪非空文本，只把纯空白值折叠为未设置
- AUTO tab 模板另行持久化，只保存模板级 AI-DYNAMIC 配置

### 首次使用引导
- 首次进入时若运行模式配置不完整，优先引导配置运行模式

## 工作流中断与恢复

- 正常节点完成后由 Runtime 自动消费该节点现有的验证结果并推进控制决策；无 artifact 的节点继续按其验证类型走既有结果读取，不要求补造产物。
- 未完成节点因 `process-interrupted / runtime-abnormal` 暂停时，composer 操作区显示“继续工作流”，恢复当前 attempt 与 ACP session 的 Runtime 控制。
- 当前节点已经 `completed/success`、但节点完成到工作流转换之间发生中断时，在同一互斥位置显示“恢复工作流”。该操作直接从已持久化的完成结果执行下一步控制决策，不重新调用当前节点 provider；若决策为 `$end` 则完成 run，否则进入下一节点。
- “继续工作流”与“恢复工作流”语义和后端入口独立，不同时展示。manual check、artifact repair、验证失败和 `ErrorBlocked` 继续使用各自既有流程，不进入完成节点恢复分支。
- Runtime execution 的 durable revision 是权威事实。provider 回调与 orchestrator 在节点边界写入时必须按同一 execution identity 单调合并 revision，旧内存快照不得覆盖更新的 phase。恢复请求携带 revision，并在 attempt 短锁与 per-run lease 内校验当前 locator，保证双击、迟到请求和并发状态变化不会重复推进。
- 性能边界：正常完成和显式恢复只增加节点边界常数次小型状态 JSON 读取/写入，不新增轮询、历史扫描、缓存、队列或依赖；锁范围只覆盖当前 attempt 的校验与 claim，不跨下一节点执行。

## 侧边栏

### 结构
1. 顶栏承载品牌区（icon + Gold Band 标题）
2. 快捷入口：快速对话、搜索
3. 功能入口：Agent 管理、上下文管理、运行模式管理、定时任务
4. 置顶区：用户置顶的会话，支持折叠/展开；与工作空间列表共用会话滚动区，整体使用对称上下分隔线包裹。展开后“置顶”标题 sticky 吸附在滚动区顶部，吸附范围受置顶容器约束，首个 workspace 标题到达时自然接替
5. 工作空间区：多 workspace 并列，每个下含 task 列表；与置顶区连续滚动，标题 sticky 吸附；hover 显示 +（新建对话）和删除按钮

顶部快捷入口与功能入口组成固定导航区，设置入口固定在侧栏底部；两者不随会话列表滚动。固定导航区采用紧凑但可呼吸的间距，优先把垂直空间留给置顶和工作空间会话列表，同时避免入口按钮被压得过紧。
- 快捷入口、功能入口、“置顶”、“添加工作空间”和设置入口使用 UI 基准字号；快捷与功能入口使用统一的紧凑按钮高度。workspace 标题使用同档字号与半粗体建立分组层级，会话项通过字重和元信息颜色继续表达层级
- 快捷入口与功能入口之间仅保留弱分隔线，不再依赖大空白制造层级；功能入口应更直接地承接快捷入口组
- “置顶”与功能入口使用同一 UI 基准字号和 `sidebar-foreground` 前景色，不能降级为辅助 `muted-foreground`；分区层级由上下分隔线、chevron 和字重表达。workspace 名称使用正文基准字号和半粗体，不强制全大写；置顶区与首个 workspace 分组之间保留更明确的换组留白
- 侧边栏 section 间距优先由局部 margin 控制，避免根容器全局 gap 与 separator margin 叠加后，把“搜索 → 功能入口”这类相邻分组撑得过空
- 功能入口四项（Agent 管理、上下文管理、运行模式管理、定时任务）与快捷入口组使用同一档紧凑组内间距；通过分隔线与图标语义区分分组，不再用额外垂直高度制造层级
- 功能入口统一使用现有 Lucide 线性图标并保持同一尺寸与描边重量；上下文管理使用 `Library` 表达角色、MCP、SKILL 等可复用资源库，运行模式管理使用 `Route` 表达工作流 / AUTO 的执行路径选择。禁止为这两个入口恢复多立方体或密集节点连线图标，避免与同组单主体图标形成视觉重量落差
- 定时任务会话的 `AlarmClock` 标识属于静态来源标识，侧边栏与会话标题统一使用 `foreground` 随主题反色；会话标题中的说明提示复用 shadcn Tooltip，视觉提示不得使用浏览器原生 `title`。
- 侧边栏宽度拖拽的交互热区与视觉分隔必须解耦：热区可以保留便于命中的透明宽度，但侧栏与主内容区之间只显示主内容圆角边界自身的低对比 1px 中性分隔线；hover / 拖动时不得把整段热区染成 primary 色带。

### 工作空间管理
- 侧边栏底部提供"添加工作空间"入口，通过系统目录选择器添加
- 添加时校验不重复，已有历史对话的工作空间自动加载 task 列表
- `conversationWorkspaces` 是会话模式工作空间列表的唯一事实源；侧边栏、搜索、运行模式、置顶和会话命令都只能解析该列表中的工作空间。`DesktopContext.repo_root` 仅用于桌面启动上下文和构造指定工作空间的 `App.paths.repo_root`，不得作为隐式工作空间注入侧边栏。
- 桌面 Runtime 启动恢复必须遍历 `conversationWorkspaces` 的全部规范工作空间，而不是只扫描 `DesktopContext.repo_root` 或 `lastConversationWorkspace`。相同规范路径只扫描一次，单个工作空间损坏只记录结构化失败并继续其他工作空间；列表为空时不回退扫描桌面启动上下文。
- 启动恢复只在桌面初始化时执行一次，逐 workspace 扫描 task/run 的 durable 状态并收敛仍为 running 的执行；不轮询、不缓存 workspace，也不调用 provider。复杂度为 `O(workspaces + Σ(tasks + runs))`，工作空间数量和本地历史规模下属于有界文件读取。
- 工作空间身份由规范路径生成 `projectId`，Windows 下解析历史 ID 时忽略盘符/路径大小写；持久化迁移按规范化路径去重，并同步迁移 `lastConversationWorkspace`、`conversationRunModes` 和 `conversationPins`。若旧状态同时存在规范 key 与历史大小写 key，规范 key 的配置优先。
- 用户状态使用 `stateSchemaVersion` 执行一次性迁移；达到当前版本后启动直接跳过迁移扫描和写盘。迁移函数同时保持幂等，重复调用不得继续改变状态。
- `stateSchemaVersion` 是共享 `StateConfig` 数据契约的一部分，按 camelCase 持久化；历史 `state.json` 缺失该字段时反序列化为 `0`，零值不额外写盘。桌面迁移模块不得声明或维护第二份影子版本字段。
- 置顶区内的工作空间名与工作空间区标题保持同一套正文基准字号、半粗体和前景色层级，避免同层级标题视觉不一致或全大写造成额外噪声
- 置顶标题、置顶区工作空间标题和工作空间区标题的文字起点与会话行标题列对齐；层级主要通过 chevron 和字重表达，不再额外增加标题左缩进
- 工作空间标题行与“添加工作空间”入口之间保持紧凑连续关系，添加入口更像工作空间区内的末尾动作，而不是远离分组的独立按钮
- 相邻工作空间分组使用 8px 的紧凑纵向间距；折叠态标题不再使用 16px 的大段留白，展开态则在完整会话列表结束后再保留同一分组间距。标题行高、会话行高和组内紧凑间距保持不变
- 工作空间标题行 hover 或键盘聚焦时显示 +（新建对话草稿目标设为该工作空间）和移除按钮；只有成功发起新会话后，该工作空间才提升为最后活跃工作空间
- 置顶区与工作空间区共用唯一 shadcn `ScrollArea`；置顶标题和 workspace 标题均使用原生 CSS sticky，不增加滚动监听，前一分组标题在下一分组标题到达时完成接替。overlay 滚动条不额外预留右侧占位，滚动时连续显示且不被 sticky 标题背景截断
- 移除前必须使用 shadcn/ui `AlertDialog` 二次确认，标题明确展示工作空间名称，正文明确说明磁盘文件和历史会话不会删除；弹窗使用紧凑确认规格（最大宽度约 480px、24px 内边距、小号正文与按钮），避免在桌面主界面中过度抢占视觉；请求完成前禁止重复确认、取消和关闭，侧边栏列表只在后端成功返回后更新。
- 移除仅删除会话侧栏持久化记录及其运行模式、置顶和最后活跃引用，不删除 task/run/session 或工作空间磁盘文件；若当前打开的会话属于被移除工作空间，成功后返回会话主页并选择后端返回的下一个活跃工作空间。
- 工作空间列表和最后活跃工作空间持久化到 state.json；最后活跃工作空间在列表中置顶；应用启动后自动展开最后活跃工作空间，成功新建/重跑会话后通过显式 reveal 请求自动定位、置顶并展开对应工作空间；仅查看历史会话或切换 composer 草稿目标不改变最后活跃工作空间。工作空间展开状态属于侧栏瞬时用户意图：composer 草稿切换、追问生命周期事件和侧栏 VM 刷新只更新各自领域数据，不得覆盖用户手动收起状态
- 工作台观察期不提供产品内模式切换；会话侧工作空间状态继续独立管理，显式 deep link 进入旧工作台时沿用旧 UI 的单 workspace 状态

### 会话行展示
- 标题（自动生成或手动修改）
- Workflow/AUTO 使用状态小圆点（绿/红/黄）；Direct 使用 Agent icon。两种标识必须占用相同宽度的身份槽位，使标题文字起点严格对齐；Direct icon 使用紧凑尺寸，不得挤占标题空间。Direct 存在当前活跃 turn 时，在 Agent icon 外显示轻量主色旋转环，结束后恢复静态 icon；不使用成功/暂停/失败颜色表达单轮结果。
- 相对时间统一来自 task 行的 `lastActivityAt`（分/时/天/周/月/年；Workflow/AUTO 运行中不显示）。该字段由后端在会话元数据活动时间、创建时间和所有 run 的 `updatedAt` 中取真实时间最大值，不能由前端按运行模式重新选择时间源。紧凑时间区间必须连续：不足 1 分钟显示“刚刚”，1–59 分钟显示 `m`，1–23 小时显示 `h`，1–6 天显示 `d`，7–29 天显示 `w`，30–364 天显示 `mo`，365 天起显示 `y`；不得在周/月或月/年边界产生 `0mo`、`0y`。
- hover 时在行尾显示重命名 / 置顶 / 删除操作；未 hover 时不为操作按钮预留占位，长标题只占用标题和时间可用区域
- 删除会话前必须弹出不可撤销确认；确认文案明确说明将删除 `~/.gold-band` 下对应 task 目录，并在系统支持时优先移入回收站
- 如果会话仍有运行中的 run，后端拒绝删除并提示用户先停止
- 滚动容器必须约束内部内容宽度，长标题只允许在会话行内部截断，不允许把相对时间或 hover 操作撑出侧边栏视口
- Workflow/AUTO 展开某个会话的 run 历史时，侧边栏只保留一个会话处于展开状态；展开新会话会自动收起旧会话，避免多个会话下相同 `run-00x` 编号同时出现造成误读。Direct 是一个持续对话，不展示或展开 run 子列表。
- run 选中态必须使用 `projectId + taskId + runId` 组合身份判断，不能只比较 `runId`；不同会话中同名 run 只允许当前会话对应项显示选中态

### 排序规则
- 最近排序：同一 workspace 内所有 Direct、Workflow、AUTO task 统一按 `lastActivityAt` 倒序。run 列表也按 `updatedAt` 倒序，task 的 `latestRun` 指向最近有活动的 run，保证排序、状态点和界面相对时间使用同一份活动事实。
- 时间比较必须先把内部 Unix 秒时间戳（如 `1780000000Z`）、RFC 3339 和历史本地日期时间归一化为时间值；禁止直接按原始字符串排序，否则混合格式会产生错误顺序。
- 置顶排序：手动拖拽可调
- 置顶会话在原工作空间下重复展示

## 状态规则
- Workflow/AUTO 状态 = 最新 run 的最终状态；成功 = 绿色，失败/异常/停止 = 红色，暂停 = 黄色，运行中不显示时间
- Direct 不用 run outcome 表达会话身份，不展示成功/暂停/失败色点。旋转环必须消费后端 task 级 canonical activity：同时覆盖首轮 runtime active、completed run 上的 same-session ACP follow-up 和 cancel requested；禁止只判断 `latestRun.status`，因为 Direct 后续追问期间底层 run 仍可能保持 completed。
- 高频 ACP session update 必须直接携带从 per-attempt prompt control registry 投影的轻量 `activity`，包括 `starting / accepted / running / cancel-requested`；prompt 终态用显式 `null` 清除。该投影只读内存控制状态，不得为侧边栏圆环重建完整 lifecycle、session 或 timeline。前端优先消费此字段，并仅对旧的无 `activity` 事件回退到 lifecycle 投影。
- App shell 必须跨当前页面与工作空间全局监听 run lifecycle 终态事件，并使用完整 `projectId + taskId + runId` locator 只更新普通工作空间区和置顶副本中命中的 `status/outcome`；后台 run 完成不得因为用户正在查看另一工作空间而遗漏。该事件不得重新请求或替换完整 `ConversationSidebarVm`，也不得加载后台 run 的 session tree、timeline 或正文；只有事件命中当前打开 run 时才局部刷新当前 `ConversationRunVm`。已投影的 terminal 状态不得被迟到的非终态更新回退。

## 性能与后台刷新

- 会话主页与消息流分别消费 Theme Contract v2 的 `panel`、`composer`、`message-user`、`message-assistant`、`message-disclosure`、`runtime-control`、`activity`、`tool-card` 和 `permission-card` role；Markdown 标题密度与 ACP 卡片结构仍由产品交互规则约束，主题不能改变 DOM、信息层级或业务状态。
- 用户消息中的系统提示/运行上下文折叠块统一使用 `message-disclosure`，AI 输出判定与控制产物统一使用 `runtime-control`。两者的背景、前景、完整边框、圆角、材质和 elevation 由主题 recipe 声明；展开状态、内容分隔、joined artifact 操作、focus ring 与 invalid/destructive 变体仍由共享组件负责，组件不得读取 `themeId` 或写死正常态主题色。
- 思考、工具审计批次和上下文压缩等非正文结构行消费 `activity`；两个内置主题的折叠 Activity 摘要统一呈现为无背景、无边框、无圆角的一行文本，静态态使用 `muted-foreground` 弱化文字，hover、键盘 focus 或展开态只切换为普通 `foreground`，不得使用主题强调色或重新增加整行背景。权限申请与 Elicitation 等等待用户介入的表面消费 `permission-card`，外层统一使用透明底、hairline 边界和零 elevation，状态强调限制在选项、按钮、错误或选中局部，不允许铺满整张决策卡；权限卡中的 Allow 系列选项静态文字使用 `accent-foreground`，hover/focus 复用 Elicitation 选项的 `accent/60` 背景与强调边界，Reject 保持中性；每轮文件变更摘要复用 shadcn `Card` 的 `card` role。会话树弹层与选择器分别复用 `popover`、`input`，不得在业务组件重新声明完整背景、perimeter、圆角或 elevation。
- AI 正文后的复制操作属于消息级次要动作，使用 prompt-kit `MessageActions` 与 shadcn `Button` 的紧凑图标尺寸；不得以空白操作行拉大正文与下一条 Activity 之间的垂直间距，同时保留 Tooltip、键盘焦点和复制完成反馈。
- 会话内容区标记稳定 `conversation` wallpaper surface。运行时只预加载当前可见槽，资源失败或 performance 档关闭壁纸时回退语义背景色；overlay 位于内容下方，不在每条消息重复创建合成层。
- `ThemeAssetsContext` 只提供低频图标 descriptor，不保存二进制资源，也不承载流式消息状态；主题或明暗切换不得导致已完成 Markdown 重新解析。

- Conversation 模式是桌面端主路径，输入与会话流式渲染期间不得启动旧 Workbench 的任务/工作流/round detail 周期刷新。
- 旧 Workbench 的 10 秒可见窗口刷新与 30 秒隐藏窗口刷新只在 Workbench 模式启用；切回 Conversation 时必须清理该 interval，避免周期性文件扫描、Tauri IPC 与 React 大状态更新抢占会话输入主线程。

## Direct 快速会话

- 快速会话模式固定按 `Direct / 工作流 / AUTO` 排列；三种模式各自保留配置，切换模式不清空正文和附件。
- Direct 配置区使用 Agent icon 列表。当前 Agent 展示 icon + 名称，其他 Agent 只展示 icon；可用 Agent 与不可用 Agent 分别保持其注册表内的既有顺序并连续排列，两组同时存在时使用一条低对比竖线分隔。不可用 Agent 保留诊断提示但不能选中。
- Direct 没有任何可选 Agent 时，空状态在提示文案旁展示紧凑的“+”按钮；按钮使用现有会话导航进入 Agent 管理页，用户添加完成后可返回继续当前快速会话草稿。
- Direct 的模型和权限模式位于 composer 右下角、发送按钮之前，不复用 AUTO 的大配置面板；两者的空选项统一显示为“不指定”，发起会话前允许在具体值与“不指定”之间切换。
- composer 主体宽度随右侧内容区增长，但保留桌面端可读上限和响应式左右 gutter；底部附件/工作空间组与 Direct 模型/权限/发送组按组参与换行，选择器允许在合理最小宽度内弹性收缩。窗口变窄、侧边栏展开、系统字体或显示缩放增大时，后组应完整下移到下一行，禁止与工作空间控件重叠或溢出输入框。
- Direct 不在运行模式管理页出现，也不展示指向该页的“去配置 / 修复”按钮；Agent、模型、权限和对应校验均在快速会话 composer 内闭环。
- Direct 的模型和权限记忆范围是 `workspace + agentType`；切换 Agent 时恢复该 Agent 在当前 workspace 上一次使用的模型和权限。
- 切换 workspace 后再返回时，必须恢复该 workspace 当前 Direct Agent 及其模型/权限；其他 workspace 的选择不得覆盖当前 workspace。切换期间 composer 的 workspace 与运行模式配置由同一个 App 层 workspace key 驱动，不保留组件内第二份 workspace 选择状态。
- Direct 会话创建后 Agent 身份不可修改；更换 Agent 等价于创建新的 Direct 会话。会话内模型与权限模式分别使用独立显式 override：未指定时不干预 Agent 当前配置，选择具体值后不再允许回到“不指定”，但可以继续切换其他具体值。
- Direct 侧边栏 task 行使用 Agent icon 代替 run 成功/暂停/失败状态点；当前 turn 活跃时由 task 级 activity 在 icon 外显示旋转环，相对时间来自 `lastActivityAt`。工作流和 AUTO 继续使用 run 状态点。
- Direct task 行点击后直接进入最近会话，不渲染 `run-00x` 子列表；底层 run 仅作为内部执行与存储结构。
- Direct 的置顶区、workspace 区和搜索结果使用同一 Agent identity VM，不允许前端组件自行从 metadata 重复推断。
