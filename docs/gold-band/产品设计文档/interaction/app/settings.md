# 桌面客户端设置页

## 1. 一句话定义
设置页用于调整桌面端语言、高级能力与个性化偏好；个性化按外观、字体、头像的顺序组织。

---

## 2. 页面入口
进入方式：
- 点击左侧底部 Settings / 设置
- 使用系统菜单中的 Settings
- 可选：快捷键打开设置

---

## 3. 页面结构

```text
┌──────────────────────────────────────────────────────────────┐
│ 面包屑：设置                                                   │
│ 标题：设置                                                     │
├──────────────────────────────────────────────────────────────┤
│ 个性化                                                       │
│   设计风格：当前主题摘要；点击后在主题抽屉中选择                  │
│   明暗模式：跟随系统 / 浅色 / 深色                              │
│   视觉效果：仅主题声明质量档能力时展示                            │
│                                                              │
│ 字体                                                         │
│   界面 UI / 编辑器分别跟随主题或选择本机字体与自定义字号          │
│                                                              │
│ 头像                                                         │
│   Agent / 个人紧凑设置行，主题头像、最近头像、上传裁剪与头像框    │
│                                                              │
│ 语言                                                         │
│   语言选择：中文 / English                                    │
└──────────────────────────────────────────────────────────────┘
```

---

## 4. 主题包与外观选择

### 4.1 选项
当前支持：
- Gold Band：默认设计风格，采用类 OpenAI 的白/近黑编辑界面；浅色以纯白画布、墨黑主操作和青绿色焦点建立层级，深色以近黑画布、克制灰阶和同一青绿色焦点保持一致。
- 技术中性：更克制的无彩工具风格，浅色与深色都只让业务状态使用彩色。
- 液态玻璃：浅色 Liquid Glass 与深色 Nocturne Glass，使用可被 backdrop 采样的黑白银灰光场、低不透明表面、方向性镜面高光和明暗内缘近似液态折射；完整效果保留液态层，性能优先关闭镜面层并降低采样成本。背景与主操作不得使用蓝色，彩色仅保留给业务状态。
- 新粗野主义：以纸白/炭黑底、紧凑圆角、黑色主操作、单一珊瑚强调和有节制的硬投影表达克制的新粗野主义；禁止整屏高亮黄、所有容器统一粗边框或每个组件同时投影。
- 明暗偏好独立为 `跟随系统 / 浅色 / 深色`；每个正式主题包必须同时提供浅色和深色，`跟随系统` 不跨主题包切换。

### 4.2 行为
- 设置保存到本地用户偏好。
- 设置页主体只展示当前生效主题摘要，并在摘要右侧保留明确的“选择主题”按钮；点击按钮使用 shadcn/ui Sheet 打开主题抽屉，完整主题包列表只在抽屉内展示。
- 点击抽屉中的主题包预览卡保存稳定 `themeId`、关闭抽屉，且不会改变当前 `colorScheme` 偏好。
- 选择 `跟随系统` 时只保存 `colorScheme = system`；操作系统变化只重新解析当前包的 light/dark，不写回偏好。
- 视觉质量选择按 `themeId` 隔离记忆；切换到不声明该能力的主题时隐藏控件且不制造伪性能档。
- 后端完成校验和原子持久化后返回 canonical `appearance + personalization`，前端以返回值收敛并应用根属性。
- 所有主题选择后立即预览，不需要重启。

### 4.3 UI 形式
设置页采用一个主工作面，内部用 section 和低对比分隔线组织外观、字体、语言，不再将每个设置组做成独立大卡片。

外观区域使用当前主题摘要与两行紧凑选项；完整主题网格采用右侧抽屉渐进披露：

```text
外观
  [ 当前主题：Gold Band     当前生效              选择主题 ]
  明暗模式                                      [ 跟随系统 v ]
  视觉效果（仅支持的主题）                    [ 完整效果 v ]

点击当前主题
  主题抽屉
    [ Gold Band ] [ 技术中性 ]
    [ 液态玻璃 ] [ 新粗野主义 ]
```

视觉规则：
- 主题卡只展示包名、当前明暗方案的视觉样本和明确选中态，不展示 token、目录或实现说明。
- 当前选中主题只用 primary 低透明背景和弱边框强调，不使用重阴影或大面积色块。
- 当前生效主题除轻量背景与边界外，还必须显示 `primary / primary-foreground` 配对的对勾状态胶囊；不能只依赖整张卡片边框让用户猜测选中项。
- 所有主题都遵循同一布局层级，主题 token 只负责换色，不改变设置页结构。
- 顶部 `通用 / 个性化 / 高级` 使用共享 shadcn Tabs 默认变体：tab track 必须使用 `secondary` surface，并以 `border` 语义的内描边与工作区分层；不得使用可能与浅色工作区同值的 `muted` 作为唯一背景。选中 tab 继续使用 `background` surface 与轻量阴影。该规则同时适用于项目中其他默认分段 Tabs，明确使用 `line` 或透明变体的场景除外。
- 每个选项自身拥有胶囊边界，或 Tabs 容器已经显式提供边界时，必须使用共享 `bare` 变体清除默认 track、padding 和 ring；不得仅用 `bg-transparent` 覆盖背景而遗留外层内描边。Agent 胶囊选择器与 Round 详情胶囊 Tabs 均遵循该规则。
- 文本选区由独立的 `text-selection` / `text-selection-foreground` 主题 token 管理，普通文本、Markdown、输入框和 composer 必须共享同一规则。两套深色使用可辨识的中灰选区与白色文字，不得继续复用与内容面接近的中性 `primary/30`；基础 Input 不再设置局部 selection 覆盖。
- Gold Band 遵循中性面优先：大面积背景使用纯白与 `#fafafa / #f5f5f5` 建立层级，墨黑承担主操作，OpenAI Teal 只用于焦点、成功路径和少量强调；边框使用 `#e5e5e5` 发丝线，不用彩色边界给整窗染色。
- 科技灰通过 `#ffffff` 主内容、`#f3f3f3` 侧栏、`#e7e7e7` 选中面和 `#e5e5e5` 边界建立层级；正文与主操作使用石墨灰，冷蓝只承担运行状态。禁止给文字、图标、边界或大面积 surface 注入蓝灰、暖黄、米色或古铜金色偏。
- 科技灰文字层级固定为 `#171717` 深黑标题、`#2b2b2b` 正文、`#666666` 辅助信息和更浅的禁用/占位状态；欢迎语等页面视觉锚点必须使用 `title` token，不得使用带透明度的正文色替代。侧栏导航、分组标题和任务标题属于主要信息，统一消费 `sidebar-foreground = #171717`，只有时间、空状态等元信息使用 `muted-foreground = #666666`。主消息阅读区保持纯白，科技灰侧栏与会话标题栏共同使用 `#f3f3f3` 框架 surface。
- 所有内置主题方案均保留独立 `content-header` 语义接口，让会话标题栏与侧边栏组成连续应用框架，并通过轻量底边界与消息阅读区分层；不得在标题栏额外包裹卡片、嵌套灰块或投影。
- 当前主题摘要按设置内容区宽度响应；主题抽屉内的主题卡网格按抽屉容器宽度在两列与单列之间切换。选项行允许标题和 Select 换行，但不得制造横向滚动或逐字纵排。
- Liquid Glass 的 Dialog 必须使用较高不透明度的 `popover` 表面并保留受限 blur/saturate、背景亮度/对比度、镜面高光和边缘阴影；不得复用低不透明度 card 造成背景文字直接穿透。非液态主题的 Dialog/Popover 保持主题包提供的实底表面。

---

## 5. 字体选择

### 5.1 选项
当前支持：
- “界面 UI”内置默认字体（`app-default`）：优先使用内置 `Gold Band MiSans`，并在系统缺字时回退到 `MiSans`、`Microsoft YaHei UI`、`PingFang SC`、`Noto Sans CJK SC`、`Source Han Sans SC` 等系统字体。
- “编辑器”内置默认字体（`editor-default`）：使用 `JetBrains Mono`，并回退到 `SFMono-Regular`、`Consolas` 等系统等宽字体。
- 本机字体：从系统已安装字体枚举加载，用户可直接选择真实字体 family。

### 5.2 行为
- 字体区分为“界面 UI”和“编辑器”两个 shadcn Collapsible 展开栏；展开状态写入 `sessionStorage`，只在当前应用会话内记忆，不进入用户配置文件。
- 字体与字号统一保存在 `PersonalizationPreference.typography`。界面 UI / 编辑器字体分别使用 `source: theme | local`，字号分别使用 `source: theme | custom`；不得通过值恰好等于 14/12 推断继承。
- UI 基准字号允许 `12–18px`；编辑器字号允许 `10–18px`，步长均为 `1px`。输入时只更新根级 CSS 变量进行即时预览，失焦后保存 `source: custom`；点击恢复时保存 `source: theme` 并立即展示当前主题预设，禁止写死 14/12、使用浏览器 zoom 或新增 localStorage 旁路。
- UI 字号控制侧栏、设置页、聊天正文、Thought 与普通 Markdown，并通过派生 token 覆盖紧凑说明、徽标和时间等层级；聊天行内代码与代码块只切换等宽字形，字号继续从 UI 基准派生。
- 编辑器字体和字号只覆盖所有 CodeMirror 文件查看/编辑、运行产物、Markdown 编辑器以及 Git/本轮 Diff；技术标识使用等宽字体不等同于消费编辑器字号。
- 全局字重语义采用轻量层级：正文 `400`、常规强调 `400`、标题/强强调 `500`、最高强调 `600`。保留视觉层次，不按页面机械替换字重类，也不通过错配字体文件伪造较细字重。
- 每个展开栏内部提供字号、内置默认字体入口和本机字体下拉列表，避免两个领域的设置混排。
- 选择后立即应用到全局 UI 字体 token。
- 字体切换必须覆盖导航栏、面包屑、任务 requirement 预览与完整需求正文等常规阅读文本；只有日志、代码块和工作图技术标识允许继续走 mono token。
- Tauri 桌面端通过 `get_system_fonts` 枚举系统字体；浏览器调试模式优先使用 `queryLocalFonts()`，不可用时回退到常见系统字体探测。
- 字体示例区带独立的“字体预览”标签和预览容器，并用彩色示例文本强化它是预览而不是正文内容。

---

## 6. 语言选择

### 6.1 选项
当前支持：
- 中文
- English

### 6.2 行为
- 选择后立即切换界面语言，或提示重启后生效。

### 6.3 UI 形式
推荐使用下拉选择：

```text
语言    中文 v
```

---

## 7. Tauri 2.x MVP 对应实现

- 外观权威字段改为 `appearance`：`schemaVersion = 2`、稳定 `themeId`、`colorScheme = system | light | dark`、按主题隔离的 `visualQualityByTheme`。旧 `desktopTheme` 在 settings schema v5 一次性迁移后删除，不双写。
- 个性化权威字段为 `personalization`：`schemaVersion = 1`，显式保存两套排版以及 Agent / 个人头像图片与形状的 `source`。settings schema v7 一次性迁移旧字体、字号和头像选择；主题来源持续跟随当前主题，用户资产历史独立保留。
- 内置 `builtin.gold-band`、`builtin.tech-neutral`、`builtin.glass`、`builtin.neo-brutalist` 分别位于独立 `themes/*` 声明式包目录，共用 DTCG token、manifest/recipe/preset、Style Dictionary alias 解析、JSON Schema/Ajv 与 Zod/Rust 双端契约；构建产出的 Catalog、CSS recipe 和 asset manifest 是 Web 与 Tauri 的共同输入，业务组件不得读取具体主题 ID。
- 设置页先选择设计风格主题包，再选择明暗模式；`system` 只解析当前主题包内的 light/dark。Glass 声明视觉质量能力并独立记忆完整效果/性能优先，其他主题不显示该设置。
- 主题运行时只更新根 `data-theme / data-color-scheme / data-visual-quality / data-material-model`、封闭 CSS variables 与原生窗口安全底色，不请求会话、不重建 timeline 或编辑器。
- 共享 shadcn/ui、prompt-kit 与应用壳以稳定 `data-theme-role` 消费材质 recipe；主题卡在宽内容区三列，窄窗口自动单列。
- 2026-08-14 基础主题包补全：Theme SDK 已生成可提交的 `runtime-theme.json`、`builtin-theme.css`、`asset-manifest.json`、Web Catalog 与 Rust Catalog；后端保存偏好从 Catalog 能力声明判断主题存在性和质量档，不再硬编码主题 ID。当前开发节点完成 Style Dictionary 构建、TypeScript/Vite 生产构建和 Rust desktop compile check；单元/接口与浏览器交互仍由后续测试、验收节点执行。
- 2026-08-14 测试节点复验：Theme SDK 构建正例及缺失 token、alias 循环、非法 recipe、质量档越界负例 5/5 通过；Web 全量 1176/1176、Rust Catalog 2/2、旧外观迁移 2/2、偏好持久化与个性化迁移定向用例通过，生产构建和格式检查通过。内置浏览器实例不可用，仅确认 `/settings` HTTP 200，Glass 长会话视觉与 GPU 时间线仍未验收。
- 2026-08-14 覆盖率工具链收敛：与 Vitest 同版本的 V8 coverage provider 作为固定开发依赖随 lockfile 安装，并提供统一 `web:test:coverage` 入口；后续测试节点不再临时修改依赖树，覆盖率结果仍必须以该节点实际执行为准。
- 2026-08-14 液态材质模型：主题引擎新增 `solid / frosted / liquid` 封闭类型和 backdrop brightness/contrast、specular highlight、edge shadow 光学参数；Liquid Glass 1.3 使用 `liquid`，性能档关闭镜面层并降低额外光学增强，其他主题显式声明 `solid`。
- 2026-08-14 三主题真实落地：Gold Band 1.1、Liquid Glass 1.3 与新粗野主义 1.1.1 均修改独立源 token、recipe 和 manifest 后重新生成 Web/Rust Catalog；Liquid Glass 1.3 移除蓝色底与蓝色交互染色，以黑白银灰光场、降低模糊半径、提高 backdrop 对比度和四向内缘高光强化玻璃透射；`subtle` 只应用边缘材质，`elevated` 才应用完整投影，业务组件与设置页不包含主题 ID 特判。

MVP 中设置页由 `web/src/pages/SettingsPage.tsx` 实现，通过 Tauri command `save_desktop_preferences` 保存用户偏好。

当前实现规则：
- 历史实现曾保存 `desktopTheme = system | light | light-gray | dark | black`，现仅由 v5 migration 读取并映射为主题包与明暗模式。
- 语言字段保存为 `desktopLanguage`，支持 `zh-cn`、`en`。
- 旧 `desktopFont / desktopEditorFont / desktopUiFontSize / desktopEditorFontSize` 仅由 schema v7 migration 读取，迁移成功后删除；运行时和保存接口只消费 `personalization`。
- `save_desktop_preferences` 以单次设置文件 load/save 原子提交 `appearance`、`personalization`、语言、本地 Claude 和日志偏好；前端串行提交并按 latest-wins 更新 canonical 偏好，禁止清空 task/workflow/round 触发无关重载。
- 主题使用主题包卡片 + 明暗模式下拉；`system` 只在当前主题包内解析浅色/深色，声明视觉质量能力的主题额外显示质量档选择。选择后立即调用 `save_desktop_preferences` 保存并预览。
- 首次启动默认 `themeId = builtin.gold-band`、`colorScheme = system`，系统明暗只改变该主题包的方案。
- 2026-05-03 起设置页使用 Tailwind CSS v4 + shadcn/ui Card、Button、Select、Badge 等现成组件重构；主题和语言选择后立即保存并预览的行为不变。
- 2026-05-07 起设置页移除标题副文案、范围提示卡片，以及外观/语言卡片中的辅助说明，页面仅保留主题与语言控件。
- 2026-05-07 起主题选择器升级为 `Sync with OS` 开关 + 主题预览卡，浅色主题扩展为两个可选变体，并新增终端黑主题。
- 2026-05-07 起完整主题列表改由抽屉承载，设置页主体只展示当前主题摘要；`system` 会保留用户最近选择的浅色/深色变体。
- 2026-05-07 起 Gold Band 深色主题从高饱和暖金黑调整为石墨香槟方向，并保留一个内置默认字体 + 本机字体下拉的双层字体选择模型。
- 2026-07-23 起默认浅色从冷蓝铺底调整为瓷白、雾灰、石墨与低饱和矿物靛蓝：背景、侧栏、边框恢复中性色，品牌色只承载交互强调；主题预览、原生窗口 resize surface 与 CSS semantic token 使用同一验收色板，并通过 Vitest 固化 WCAG AA 对比度。
- 2026-07-23 起其余三套主题同步按同一层级原则重制：第二套浅色减少黄色铺底，Gold Band 深色拉开石墨 surface 层级，终端黑移除海军蓝大底；四套主题均由同一语义 token 接口驱动，设置页预览、原生窗口 surface 和 CSS runtime 色板通过统一测试验收。
- 2026-07-24 浅色主题命名与科技灰替换：默认浅色更名为“瓷白”且保持既有色板；原第二套浅色完整替换为“科技灰”，内部 ID 改为 `light-gray`。二次对照校准后采用 `#ffffff / #f3f3f3 / #e7e7e7 / #e5e5e5` 无彩层级和石墨交互色，冷蓝仅保留给运行状态，消除旧蓝灰色偏。主题选项的数据模型、设置页和中英文 i18n 同步删除说明字段及说明文案，只展示主题名称。
- 2026-07-24 主题抽屉密度收敛：删除说明文字后不再保留整行大摘要卡，分组标题移到网格上方，抽屉按自身宽度在单列和双列视觉样本墙之间切换；当前主题增加语义化对勾状态胶囊，解决宽面板信息稀疏和选中状态不明确的问题。
- 2026-07-24 深色主题二次校准：对照 AionUi 默认深色 token 后，将原 Gold Band 深色改名为石墨深色，并采用跨度更明确的中性灰阶与冷蓝操作色；终端黑改用墨黑、冷灰与灰紫组合。两套浅色保持不变。
- 2026-07-24 深色视觉最终校准：根据 Codex 桌面端截图取样，将两套深色统一调整为无彩黑灰体系；`primary` 不再使用蓝色或灰紫，选中、按钮和 composer 通过灰阶表达，彩色仅保留给运行、成功、权限/警告和危险状态。
- 2026-07-24 深色选中态可读性修正：侧栏导航、会话运行项和会话切换器统一使用 `sidebar-accent` / `sidebar-accent-foreground` 语义配对，禁止将选中面与 `sidebar-primary` 文字色混用，避免无彩深色主题下前景与背景同色。
- 2026-07-24 设置页响应式修正：section、主题摘要列表、单张摘要卡和主题抽屉改为分层容器查询；移除基于整窗 `md/lg` 的提前升列，窄内容区统一降为单列/纵向结构，避免主题文案逐字换行以及预览、按钮互相挤压。
- 2026-07-24 浅色 Tabs track 可见性修正：共享 `TabsList` 默认变体从 `muted` 调整为带 `border` 内描边的 `secondary` surface，覆盖设置页、会话运行模式、工作流编辑、任务筛选、运行模式管理和上下文管理；透明与 line 变体保持原设计。
- 2026-07-24 科技灰侧栏文字层级修正：导航、分组与任务标题不再复用 `#666666` 辅助文字，而是统一消费深黑 `sidebar-foreground = #171717`；选中态前景同步保持深黑，时间和空状态等元信息继续使用辅助灰。
- 2026-07-24 文本选区可见性修正：四套主题新增成对的文本选区 token，深色选区提升为明确中灰层级；移除 Input 对 selection 的局部 primary 覆盖，使普通文本、Markdown、输入框和 composer 呈现一致。
- 2026-08-02 滚动条视觉层级校准：四套主题继续通过统一的 `gold-scrollbar-track` / `gold-scrollbar-thumb` / `gold-scrollbar-thumb-hover` 语义接口驱动原生滚动容器、主题滚动容器与 shadcn `ScrollArea`。滚动条改用中性前景色的低透明叠加，静止态不得混入品牌 `primary` 或不透明辅助文字色；轨道仅在容器 hover 时提供极弱反馈，thumb 在 hover 时再适度增强。两套浅色采用 3% / 16% / 26% 的轨道、静止、悬浮层级，石墨深色采用 4% / 18% / 30%，终端黑采用 4% / 20% / 32%，保证可发现但不抢占正文与导航的视觉重心。
- 2026-07-24 胶囊 Tabs 边界收敛：共享 Tabs 新增 `bare` 变体，Agent 选择器和自带边界的 Round 详情 Tabs 不再继承默认 track ring，避免外层轨道与选中胶囊形成双重边界。
- 2026-05-08 起应用内置默认字体切换为 MiSans（前端 family 为 `Gold Band MiSans`）；设置页删除三套 CJK 预设，只保留一个默认字体卡片与一个本机字体下拉列表。
- 2026-05-08 验收修正：字体切换必须同步作用到导航栏、面包屑、任务 requirement 预览与完整需求抽屉；这些区域不再误用 mono token。
- 2026-05-07 起设置页从多张独立卡片收敛为一个主工作面，外观、字体、语言通过 section 与低对比分隔线组织；主题摘要、字体选项和本地字体预览降级为低对比选项行，避免盒中盒和浅黑色块过多。
- 2026-05-25 起设置页改为三个 tab：语言进入通用，主题和字体进入个性化；高级页展示当前更新渠道、内置更新地址、有效更新地址，支持用户持久化覆盖更新地址、恢复内置地址和手动检查更新。2026-07-30 起原“外观”tab 正式更名为“个性化”，并新增头像设置。
- 2026-07-30 个性化页顺序调整为“外观 / 字体 / 头像”；头像作为低频设置放在底部，Agent 与个人头像使用 48px 预览、短说明和紧凑形状按钮组成的响应式横向设置行，不再显示冗余“头像框”标签。
- 2026-08-14 字体区新增 UI/代码基准字号设置与恢复默认入口，并将全局 `medium / semibold / bold` 语义由 `500 / 600 / 700` 校准为 `400 / 500 / 600`，从字体系统根部降低整体视觉重量；字号写入既有桌面偏好配置，应用启动时统一恢复根级 CSS 变量。
- UI 小字号统一使用 `text-ui-nano / micro / caption / compact` 排版 token，并随 `--app-ui-font-size` 缩放。共享 `cn()` 必须把这些 token 识别为字号类，使字号与 `text-foreground / text-muted-foreground` 等颜色类独立合并；Button、Badge、CommandItem 等 shadcn copy-in 组件不得因 class 合并丢失任一语义。
- 头像系统的完整数据、存储、交互与会话展示规范见 [avatar-system.md](avatar-system.md)。
- 设置页中的问号帮助入口（如“使用本地 Claude”“记录详细日志”“开启指标上报”）统一使用随主题变化的浅色 shadcn/ui `Tooltip`，悬浮或聚焦即可展示说明文本；这些布尔开关统一采用“标题 + tips icon + switch”同一行布局，避免一部分开关右置、一部分行内导致对齐不一致；同时避免页面出现主题色 tooltip 与白底说明面板混用。
- 更新能力使用 Tauri updater：`default` 渠道内置 GitHub Release `latest.json`，`wb` 渠道内置内网占位地址；两个渠道使用不同 updater public key，用户只能覆盖 URL，不能覆盖 public key，因此两个渠道不会通过改 URL 串包更新。default 渠道的安装包、签名和 `latest.json` 由 `release-please` 创建 draft release 后在同一 GitHub Actions workflow 确保 git tag 存在并上传；该 workflow 可由 `main` push 自动触发，也可在 GitHub Actions 页面手动触发以补跑 release-please 主链路，release publish 后才对客户端 latest 检查可见。
- `wb` 渠道本地执行 `npm run build:wb` 生成 `latest.json` 时，必须优先选择与本次 `--version` 精确匹配的签名安装包；即使 `release/wb` 或构建产物目录里残留旧包，也不能把下载 URL 指回历史安装包。
- 桌面端启动后后台定时检查更新，发现新版本只更新状态并提示用户，不自动下载或安装；用户可在高级页手动检查，有新版本时再点击下载并安装；上次检查时间持久化为本地系统时区 `YYYY-MM-DD HH:MM:SS`。
- 2026-05-27 起更新提示增加三级红点：后台发现当前可更新版本后，左侧 `Settings`、设置页 `Advanced` tab、`Updates` 分组标题同时显示红点。用户进入设置页时只清除 `Settings` 红点；切到 `Advanced` tab 时只清除 `Advanced` 红点；`Updates` 红点不因进入页面消失，只有当前已无可更新版本时才自动消失。
- 红点状态按“当前可用版本号 + 分层已读版本号”计算，而不是简单布尔值：`Settings`、`Advanced` 和公告关闭状态都持久化到用户级桌面配置，并与版本号绑定；同一版本已读/关闭后不再重复提示，但一旦后台发现更高版本，三级红点和公告都会重新出现。
- 右侧主内容区顶部新增公告区：首次发现某个新版本且该版本公告尚未被关闭时，页面 header 下方展示一条可关闭公告，提示“发现新版本，可前往 设置 → 高级 → 更新”。点击“查看更新”打开轻量弹窗，明确引导用户前往设置页更新；关闭公告后仅移除公告本身，不影响三级红点。
- 可用更新快照持久化到用户级桌面配置，因此用户在发现更新后关闭应用再打开，公告、设置页状态和更新版本信息不会因为重启丢失；只要存在这份快照，高级页更新状态区就按“可更新”态展示版本信息与安装入口，而不是回退成“尚未检查”；只有后续检查确认当前已无可更新版本时，才清空这份快照与 `Updates` 红点。

## 8. 2026-06-12 指标上报地址展示

- 设置页的指标上报区域只展示一个“上报服务地址”，不展示心跳与节点详情两个接口后缀。
- `wb` 渠道默认服务地址为 `http://maling.weoa.com`，且随锁定开关一起禁止用户修改。
- 实际上报接口由客户端统一拼接：心跳使用 `/api/client-report/heartbeat`，节点详情批量上报使用 `/api/client-report/metrics/batch`。
- 保存设置时只持久化服务根地址 `metricsBaseUrl`，旧的心跳/节点完整接口地址配置不再读取。

---

## 7. 一句话总结

> 当前设置页只解决“我想用什么主题、字体和语言”，不承载任务编排、provider 配置或 workflow 编辑能力。

## 9. 定时任务运行设置

- 设置页是定时任务全局运行设置的唯一可见入口，通过 `get_scheduled_runtime_settings` / `save_scheduled_runtime_settings` 统一管理保持唤醒、完成通知和 occurrence 保留天数；保留范围固定为 `1..=3650` 天，越界返回 `SCHEDULED_VALIDATION_FAILED` 及结构化 `field/minimum/maximum/actual` 参数。
- 保持唤醒同时展示用户启用值与系统实际生效值。只有用户开启、至少一个 job 为 enabled 且应用仍在运行时才生效；平台获取失败展示 `SCHEDULED_POWER_INHIBITOR_FAILED`，但不改变任务调度与 occurrence 结果。
- Windows、macOS 和 Linux 使用 `keepawake 0.6.0` 的统一进程级 guard；Windows 走 System Power API，macOS 走 IOKit，Linux 走系统 inhibit 后端。配置允许显示器休眠，只阻止空闲导致的系统自动睡眠，应用退出时必须释放 guard。
- macOS 不是后续兼容项，而是 Task 6 的同级目标平台：实现必须保留 `objc2-io-kit`/IOKit 后端与相同的启用、失效、退出释放语义。Windows 开发机无法替代 macOS 编译或真机验证；发布验收需要在 macOS CI 或真机上补充编译和开关 smoke test。
- occurrence 默认保留 30 天。清理仅删除 SQLite 中过期的 `succeeded/failed/skipped/missed` 行，保留 `attention_required`、非终态和活动 Run 链接；Task、Run、Round、ACP 文件与产物不属于该清理事务。
- 2026-08-09 起 `ScheduledRuntimeSettings` 只在通用设置页展示；定时任务管理页移除重复入口，但继续由同一命令和状态模型服务设置页，不增加页面级副本。
