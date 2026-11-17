# 连接与终端（Connections & Terminal）

## 定位与范围

Gold Band 是 EXE 桌面产品，主路径保持任务编排 / 会话心智，不使用 command bar / slash command / 终端输入心智。在此基础上，本能力为**受管的「连接 / 终端」模块**：用户维护**用户级全局**的服务器连接配置（仅限**测试环境**），从应用内打开**本地工作目录终端**或**远程 SSH 终端**（可直连目标机，也可**经跳板网关转发**），用于登录测试服务器、查看日志与执行排障命令。

- 该模块是一个**独立、隔离**的受管能力，与源码管理、Agent / 上下文、任务编排互不覆盖；不把任意 shell 能力暴露给既有任务编排主路径。
- 它不是通用终端模拟器：不追求任意终端仿真、脚本化 / 自动化、多协议（Telnet / RDP / 串口等），只覆盖 SSH 远程接入与本地 shell 排障两种用途。
- 仅限测试环境服务器连接，**禁止生产直连**（范围约定，非技术阻断；见「环境与范围约束」）。
- 开发计划：`docs/gold-band/开发计划/终端与跳板/终端与跳板机开发计划.md`（含 WindTerm 网关 spike、分阶段实现与验收）。

### M1 范围

1. 连接配置 CRUD：用户级全局，稳定 ID、名称、host、port、username、认证方式、可选跳板引用、环境标签。
2. 本地终端：完整交互 shell，默认落在当前 workspace 根目录，可自由 `cd`；支持另开任意目录的本地会话。
3. 远程终端：目标机直连 + 经跳板网关转发（网关与目标机可分别使用不同凭据）。
4. 多会话标签页并发（上限 4–8 个）；会话**瞬态**，关标签即终止进程 / 断开连接。
5. 最近连接历史：纯元数据（连接 / 主机 / 账号 / 时间 / 环境），用于快速重连展示。
6. 认证：SSH 密钥 + 口令；口令 / 密钥口令可选择性存入系统钥匙串自动喂入；OTP / 键盘交互动态口令连接时由用户实时输入，不落盘。

### M1 暂不做

- Agent（ACP）远程执行 / 自动上服务器排障。
- SFTP / 文件浏览 / 下载。
- 受管 log-tail 面板、输出过滤 / 高亮等便利层。
- 连接配置团队共享 / 导入导出。
- 生产环境连接（能力范围外）。
- 会话持久化 / 断线自动恢复 shell 状态。
- 主机指纹人工确认 UI。
- 除「复用主机指纹不匹配」结构化错误外的增强信任管理（证书固定、CA 校验等）。
- 工作区 / 会话会话间复用同一终端等深层整合。

---

## 信息架构与入口

### 页面定位

「连接与终端」是一个**不绑定任何 workspace** 的用户级全局页面，与会话模式下的 Agent 管理、上下文管理、运行模式管理、定时任务同级。打开、管理与使用该模块**不要求先打开项目 / workspace**：

- 远程连接的管理与打开始终可用（无 workspace 时同样可用）。
- 本地终端在存在 workspace 时默认落在当前 workspace 根目录；无 workspace 时新建本地会话要求用户先选择目录。

### 一级入口

- 会话模式左侧栏固定导航区（与 Agent 管理 / 上下文管理 / 运行模式 / 定时任务同组的导航按钮）新增「连接」入口，选中态与其他导航按钮一致并高亮。
- 深度链接：`/chat/connections`（与 `personal-analytics`、`scheduled-tasks` 同一路径层级）。会话模式内从其它页面 push/replace 到该页遵循既有 `pushRoute` / `replaceRoute` 机制。
- 该页面不提供页内返回 / 面包屑；与同级页面之间通过侧栏导航切换。

### 导航与前端落点（实现指引）

- `ConversationPage` 增加 `| { kind: 'connections-terminal' }`（`web/src/types.ts`）。
- `renderConversationContent()` 增加对应分支渲染 `ConnectionsTerminalPage`（`web/src/App.tsx`）。
- `routeFromPath` / `pathFromRoute` 增加 `/chat/connections` ↔ `connections-terminal` 映射（`web/src/routes.ts`）。
- `ConversationSidebar` 固定导航区新增 SidebarButton（图标 + label），点击 `onSelect({ kind: 'connections-terminal' })`；导航高亮 key 开关增加该 kind（`web/src/components/conversation/ConversationSidebar.tsx`）。
- i18n：导航与页面文案新增命名空间 key，zh-CN 与 en 同步维护（既有侧栏 key 位于 `conversation.sidebar.*`）。

---

## 页面构成

### 连接列表

- 展示摘要列：名称、host、username、环境标签、最近连接时间；经跳板的连接需展示跳板标识（避免与直连混淆）。
- 空态：无连接时给出「新建连接」引导与「打开本地终端」入口，不展示解释性长文案。
- 行操作：打开远程终端、编辑、删除（删除需二次确认，并提示将同时删除对应已保存凭据）。
- 打开远程终端时若该连接经跳板，展示完整连接链路（本机 → 跳板 → 目标），并展示目标机环境标签。

### 新建 / 编辑连接

表单字段：

- 名称（展示用，重名校验提示；不参与身份）
- host、port（默认 22）
- username
- 认证方式：口令 / SSH 密钥；口令或密钥口令可勾选「保存到系统钥匙串」，未勾选则连接时要求输入一次（本次会话内记忆，不落盘）
- 密钥路径（认证方式为密钥时）
- 跳板（可选）：引用一个已存在的连接作为跳板网关
- 环境标签：test / stage / prod（**必填**；UI 对 stage/prod 做红标警示文案，prod 明确展示「不在支持范围」提示但不阻断演示性录入）

### 会话区

- 多标签页，每个标签承载一个终端（本地或远程）。
- 标签标题：本地会话显示目录名 / 会话序号；远程会话显示「连接名（host:port）」。
- 会话状态行（连接中 / 已连接 / 认证等待 / 已断开）与断开原因放在标签页头部紧凑区域，不占用终端文本流。
- 新建会话入口：本地终端（当前 workspace 根目录 / 选择目录）、从连接列表打开远程会话、最近连接快速重连。
- 会话上限 4–8：达到上限时禁用新建并提示；关闭标签释放 pty / SSH 连接，进程组与端口句柄必须随之清理。

### 最近连接

- 只保存元数据历史：连接 / 主机 / 账号 / 环境 / 最近连接时间，按时间倒序，供「快速重连」与列表排序。
- 连接被删除后，其历史条目不再提供一键重连，展示时移除或标记为不可达；不反写、不自动删除历史，避免旁路写依赖。

---

## 数据模型

### ServerConnection（用户级实体）

- **稳定 ID**：后端生成（沿用仓库分布式 ID 约定，如 `conn-<uuid4-without-hyphens>`），连接名称只用于展示与重名校验，不参与身份生成。
- 字段：`id`、`name`、`host`、`port`、`username`、`authMethod: 'password' | 'publickey'`、`keyPath?`、`credentialRef?`（凭据引用，见下）、`jumpRef?`（跳板连接 ID，禁止自引用 / 环引用）、`env: 'test' | 'stage' | 'prod'`、`createdAt`、`updatedAt`。
- 持久化：用户级目录 `.gold-band/` 下独立 JSON 文件（与既有用户级配置同级），沿用原子写 + 文件锁 + schema 版本化迁移；**列表接口只返回摘要**，详情按需读取。
- 引用完整性：`jumpRef` 指向的连接被删除时，引用方进入「跳板缺失」状态并持久化剔除或显式提示修复，不允许静默保持指向已删除实体。

### 凭据（OS 钥匙串，非配置文件实体）

- 口令 / 密钥口令存入系统钥匙串（Windows Credential Manager / macOS Keychain），服务名统一为 `gold-band-ssh`，账号为对应连接稳定 ID。
- 配置 JSON **永不写入凭据明文**；`credentialRef` 只表达「该连接有已保存凭据」。
- 删除连接时同步删除其钥匙串凭据。
- 认证所需私钥文件路径可写入配置（路径非秘密）；私钥 passphrase 走钥匙串，不写入配置。

### 最近连接历史（元数据实体）

- 字段：`connectionRef?`、`host`、`username`、`env`、`lastConnectedAt`；连接已删除时保留条目但不可重连。
- 持久化：独立小 JSON 文件，有界（如保留最近 50 条），仅追加 / 最近更新语义，不进入任何关键路径。

### 会话（运行时瞬态，不持久化）

- 会话只存在于运行期内存：`{ sessionId, kind: 'local' | 'remote', refs, pty/连接句柄, status }`；应用退出 / 标签关闭即销毁。
- 不维护会话恢复、快照或断线重连状态机。

---

## 安全与凭据

- 凭据默认不进配置文件；是否保存由用户显式选择，保存目标只能是系统钥匙串。
- host-key 校验策略（按已确认设计决策）：**首次连接静默接受并按主机记录指纹；复用同一主机再次连接时指纹不匹配 → 返回结构化错误 `terminal.host-key-mismatch`，前端展示错误与「忽略并继续 / 更新记录」操作**。不提供首连指纹确认 UI。
- OTP / 键盘交互动态口令：连接流程中需要时，后端发出「等待用户输入」事件，前端在会话状态区提示输入一次；输入仅存活于本次认证内存，不落盘、不进历史。
- 环境与范围约束：
  - 连接创建 / 编辑必填环境标签；列表与会话头对非 test 环境红标展示。
  - 生产连接**不在能力范围内**。产品按范围约定对待，不实现「黑名单 / 拦截器」式的技术阻断；测试环境的判定与治理属于运维侧账号与网络边界职责，客户端只做清晰、醒目的环境标识与文案约束，避免给用户「客户端能管住生产」的错误安全感。

---

## 会话生命周期与状态

- 状态：`connecting → authenticated → shell-ready / error`；`waiting-otp` 为认证中间态；断开为终态 `closed`。
- 终止：关标签、页面离开（该页会话随页面销毁）、应用退出任一途径都必须同步终止本地 pty 进程组 / 远程 SSH 连接与占用的转发通道。
- 异常：连接失败 / 中途断开用结构化错误展示原因（见错误码），断开后标签保留最近一屏输出但不可继续输入，用户可「重连」开新会话（不恢复原 shell 状态）。
- 本地会话与 workspace 的关系：本地会话默认 cwd = 当前 workspace 根目录；workspace 删除或不可达时已开本地会话继续运行至标签关闭，新开本地会话则要求重新选择目录。

---

## 认证形态（用户可见）

| 认证方式 | 凭据来源 | 自动喂入条件 |
| --- | --- | --- |
| SSH 密钥 | 私钥文件 + passphrase | passphrase 已存钥匙串时自动喂入；否则连接时输入一次 |
| 口令 | 口令 | 已存钥匙串时自动喂入；否则连接时输入一次 |
| OTP / 键盘交互 | 动态口令 | 一律连接时实时输入，不保存 |

---

## 技术方案摘要

- 后端内嵌 SSH 客户端（候选 `russh`）：口令 / 密钥 / 键盘交互 / host-key 校验 / `direct-tcpip` 嵌套跳板；不依赖系统 `ssh.exe`。
- 本地终端 host PTY：候选 `portable-pty`（Windows ConPTY / Unix forkpty），以受管子进程方式创建；Windows 下不得弹出独立控制台窗口（沿用 `src/process.rs` 隐藏控制台约束，交互终端是本模块受管子进程的显式形态，与既有非交互 `background_command` 分界清晰）。
- 前端渲染：`xterm.js` + fit 插件，终端容器组件化封装；不手写终端渲染 / 输入基础控件。
- 传输通道：会话字节流采用**二进制安全的分块推送 + 前端有序消费**抽象（xterm 层只消费合并后的字符串）。载体（阶段 B 已定）走既有 `app.emit` 事件（`gold-band://terminal-output` / `terminal-exited`），payload 为 `#[serde(tag="event")]` 的 `TerminalEvent`，输出 base64 分块、输入 base64 解码上行；背压来自 pty reader 阻塞（有界缓冲，不无界积压），不允许 `Vec<u8>` 走 JSON 数字数组之类的高开销路径。高流量用例基准（单会话 `tail -f` 大流量日志、4–8 会话并发）留待阶段 F 验收，届时如 event-emit 不达标再评估切换到 `tauri::ipc::Channel`。
- 跳板段采用**传输适配器抽象**：会话描述为「目标连接 + 可选跳板连接」，跳板段接口统一为 open → authenticated channel → 转发到目标；实现分支（标准 SSH 网关 direct-tcpip 嵌套 / 公司 CLI 适配器）由 WindTerm 网关 spike（见开发计划阶段 0）结论决定。
- 依赖版本与选型细节以 spike / 技术核验结论为准，遵循「先验证、再引入」。

---

## 错误与本地化

- 后端只返回结构化错误 `{ code, params }`（沿用 `CommandErrorVm` / `RuntimeErrorInfo` 契约），不返回任何对客文案。
- 前端 `displayAppError` 按 code 本地化展示，zh-CN / en 同步。
- 主要错误码见开发计划第 7 节草案：连接读写、凭据失效、认证失败、host-key 不匹配、连接超时 / 中断 / 网关不可达、跳板转发失败、OTP 等待、本地 shell 启动失败、会话数超限。
- 「等待用户输入 OTP」是连接中事件而非错误，需单独通道表达，不落入错误码文案。

---

## 性能与资源边界

- 连接配置为用户级低频实体（预计数十内）：列表只返回摘要，详情按需读取；不参与会话 / run / 任务关键路径。
- 终端字节流：分块 + 有界缓冲 + 背压；慢消费者丢帧保护采用「丢弃渲染帧、保留最新状态」，禁止无界积压；前端按动画帧 / 时间窗合并写 xterm，避免逐块重排。
- 会话并发上限 4–8，每个会话独立进程 / 连接；标签关闭即终止并释放句柄。
- 高流量日志分块窗口与合并策略需在实现阶段定义基线并用生产构建 / 时间线验证；此条遵循 `docs/gold-band/rules/frontend-performance.md`。

---

## 与其它模块的边界

- 源码管理：不把任意终端 / shell 命令能力暴露给源码管理入口；本模块独立提供受管连接与终端，两者不互操作。
- Agent / 上下文 / 运行模式 / 定时任务：不消费、不修改这些模块的数据；连接与终端数据不外泄给 ACP / 任务运行时。
- 个人数据分析：最近连接历史是本地元数据，不作为个人数据分析的数据源（MVP）。
- Workbench（旧 UI）：本模块属于会话模式页面；不向 Workbench 主模块导航暴露连接管理入口。

## 产品决策收敛（2026-09-03 确认，落地中）

> 本节记录后续对交互的再架构确认；与上文若有出入以下文为准。

- 终端会话为**全局共享**模型：任意入口（workspace dock 终端标签 / 独立连接页）显示同一会话集合，共用全局 4–8 上限，关任意处即释放 pty/SSH 与额度（对应原「多会话标签 + 会话上限」小节）。
- 本地终端落点（三级回退）：当前 active conversation workspace 根 → 全局 repo 根 → 目录选择（不再仅依赖全局 repo 根，解决「与项目不挂钩」）。
- 终端头部常驻显示**完整工作路径**（monospace、可复制），会话标签仍为 basename（解决「不显示路径」）。
- 终端主承载从「独立页」迁入 conversation workspace 视图的 **`RightWorkspaceDock` 新增 `terminal` 资源标签**：顶栏「终端」按钮 + 全局快捷键 Cmd/Ctrl+T 打开；与 agent-transcript 等面板同级分栏（解决「层级深 / 项目↔终端切换不便」）。
- 独立「连接」页定位：连接管理为主，保留共享终端面板（与 dock 同一会话），全局活动终端概览为可选细化。
- 交互规则：主路径不在独立页 / 连接管理里堆终端心智；打开终端是项目视图内的一步操作。

## 动态口令 / 错误提示 / 独立窗口收敛（2026-09-03 确认）

- 动态 / 一次性口令（含跳板机策略轮换的密码）：新建连接时口令可留空不保存，连接时由 SSH 层键盘交互实时弹窗询问，不落盘（`AuthPrompt`/`provide_terminal_auth`，已修复前置校验误挡该路径的 bug）。
- 跳板机内部（SSH 认证已成功后）菜单/脚本再次要求密码：视为普通终端文本 I/O，直接在终端窗口输入，不做识别/接管引擎；表单内提供辅助说明引导这一习惯。
- 连接管理保存/删除失败的错误提示必须在触发它的弹窗内部展示、常驻直到用户手动关闭，不允许被弹窗遮挡或自动清除；错误文案统一走结构化错误码 `errors.<code>` 渲染，后端只返回 code + params。
- 拖出独立窗口（终端会话 / 连接管理页）：范围限定这两处，不泛化到其他 dock 面板；通过菜单动作（非真实拖拽手势）触发 Tauri 独立 `WebviewWindow`；终端拖出后主窗口对应标签消失，独立窗口关闭即等同于关闭该终端标签（结束会话、释放上限额度），与现有"关闭=终止"心智一致；连接管理页拖出是"搬走"（主窗口视图清空，避免双写竞态），关闭后自动归位。

## 独立窗口实现细节（2026-09-03）

- 新增后端命令：`open_detached_window(kind, session_id?)` / `close_detached_window(label)` / `list_detached_windows()`（`src-tauri/src/detached_window.rs`）。窗口标签形态 `detached-<kind>[-<sessionId>]`，`WebviewWindowBuilder` 运行时创建，加载 `index.html?window=<kind>[&session=<id>]`。`DetachedWindowRegistry` 跟踪已开窗口；`spawn_cleanup` 关闭所有 detached 窗口；`emit_to("main", gold-band://detached-window-closed)` 通知主窗口重新吸附。
- capability：新增 `src-tauri/capabilities/detached.json`，`"windows": ["detached-*"]`（glob 匹配），含 `core:default` + 窗口创建/关闭权限。主窗口的 `default.json` 仍限 `["main"]`，互不影响。
- 前端隔离入口：`web/src/main.tsx` 依据 `window.location.search`（`parse_detached_route`）分流：detached 则渲染 `<DetachedApp>`（Terminal 或 Connections 的独立 UI，不含完整 Shell/`WindowCloseCoordinator`）。会话/事件均复用全局后端状态与 `app.emit` 广播，无需额外同步。
- 交互：终端会话标签栏每个标签新增「在新窗口打开」（ExternalLink 图标）；连接管理页标题栏「在新窗口打开」。拖出后该会话从主窗口面板过滤隐藏（`detachedIds`），detached 窗口关闭（事件）后重新吸附出现。detached 窗口内提供「收回到主窗口」（`close_detached_window`，会话不结束）与原生关闭（=结束会话）。

## 跨窗口会话历史持久化（2026-09-03）

- 问题：独立（pop-out）窗口在「收回到主窗口」时，会话历史丢失，如同新建。根因：`terminalHistory` 是各 webview 进程内的 JS 模块；主窗口仅在会话可见时由 `TerminalView` 追加，detached 窗口关闭（其进程销毁）时丢失 detach 期间产生的输出。跨窗口的历史无法靠单进程 JS 状态承载。
- 修复：把历史缓冲上移到**后端**（单一事实源，所有窗口共享）。`terminal_session_state_for` 构建 `TerminalHistoryStore`（`Arc<Mutex<HashMap<sessionId, Vec<u8>>>`，有界 512KB），事件 sink 对每个 `Output` 分块 decode 后追加；会话 exit/failed 时清理该 session 的缓冲。新增命令 `get_terminal_history(session_id)` 返回 base64。前端 `TerminalView` 挂载时异步拉取历史回放（移除模块级 `terminalHistory`）。
- 回归：Rust `commands::tests::terminal_history_*`（累积/回环/有界）；前端 `terminal_view_lifecycle`「挂载时回放后端历史」、`detached-route.test.ts`。

## 渠道能力门控 + 连接→对话设计（2026-09-03）

### 渠道能力：远程连接仅内网
- 终端（本地）所有渠道可用；远程连接 + 经跳板仅内网（wb 渠道 true，default/外网 false）。
- 落地：渠道级布尔 `remoteConnectionsEnabled`（`configs/channels/*.json` → `build.rs` 发 `GOLD_BAND_REMOTE_CONNECTIONS_ENABLED` → `channel.rs` → `AppInfoVm.remoteConnectionsEnabled` → `FeatureAvailabilityProvider`/`useFeatureAvailability`）。
- 门控面：后端 7 个远程命令 `ensure_remote_connections_enabled()` 守卫（`connection.channel-not-supported`）；前端 `TerminalPanel` 远程下拉+连接加载、侧栏「连接」入口、`ConnectionsTerminalPage`（外网显示 disableNotice）。

### 连接→对话（附连接给 agent，agent 自己连接操控）——设计定稿
- **核心**：只给 agent 一个**非密钥连接 spec**（标识 + 目标），由**应用暴露的工具（MCP）**驱动 agent 连接操控；密钥永不出应用，agent 不重造 SSH。
- **工具承载**：作为附加 MCP server 注入该 run（复用 `WorkerInvocation.mcp_servers` 槽）：`list_connections`/`open_connection`/`send_input`/`read_output`/`close_connection`。
- **会话级默认上下文**：连接绑定为该对话的持久上下文，每轮经 `WorkerInvocation.extra_hidden_sections` 注入隐藏上下文；会话顶部显示常驻「连接参考」卡（Popover 悬浮看完整 spec），可移除（移除=从上下文剔除）。
- **新/已有对话**：新=创建时绑定；已有=立即绑定为该对话默认。
- **agent 打开的会话与应用共享**（同一 `TerminalSessionManager`，用户可审阅/接管）。
- **OTP/动态口令**：agent 驱动需口令时，应用向主窗口弹 `AuthPrompt`，agent 的 open 挂起+超时；超时未响应 → 结构化 `terminal.auth-required`。
- **已落地地基（2026-09-03 连接→对话 阶段 1–2）**：`ConnectionSpec`（非密钥子集 `{id,name,host,port,username,authMethod,env,jumpRef}`，绝不含 `key_path`/秘密）+ `describe()` 渲染；回归 `connection_spec_is_non_secret_and_describes_conn_target`。`ConversationPromptInput.context_connections` 与 `RunState.connection_specs`（`#[serde(default, skip_serializing_if=Vec::is_empty)]`）字段已就位，run 级持久绑定地基已加。
- **已推进（2026-09-09 连接→对话 阶段 3）**：注入点已接通——`build_worker_invocation` 读 `RunState.connection_specs` 经 `provider::connection_hidden_sections` 写入 `WorkerInvocation.extra_hidden_sections`（空 spec → 空节，无行为回归）；`prepare_acp_prompt_for_attempt` 在 `.clear()` 后从 `run.connection_specs` 重注入，跟随连保持连接上下文。回归：`node_executor::tests`（`worker_invocation_injects_bound_connection_sections` / `worker_invocation_emits_no_connection_sections_when_none_bound`）、`orchestrator` 跟随连重注入断言。注：`context_connections → run.connection_specs` 的绑定写入随 composer / MCP 工具（阶段 6/7）落地，当前注入从 run 读取、暂无写方，属占位分支。

#### 阶段4：连接 MCP 工具 server（设计定稿，2026-09-09）

- **架构**：应用内 streamable-HTTP MCP server（方案 A）。应用起 localhost HTTP 端点（`http://127.0.0.1:<port>/mcp`），注入 `{"type":"http","name":"gold-band-connections","url":"http://127.0.0.1:<portal>/mcp"}` 到 `WorkerInvocation.mcp_servers`（复用现有槽，按 `run.connection_specs` 非空才注入）。工具回调**直接进程内访问** `ServerConnectionStore` + `TerminalSessionManager`/`SshSessionManager` + OTP 事件总线，无子进程/IPC。选 A 因为连接/会话必须由常驻 app 进程管理，`http` 端点让工具在进程内回调，贴合「桌面应用心智」；跨进程 stdio（方案 B）需父↔子 IPC，子进程够不到父进程内存态 manager，故弃。
- **协议**：MCP JSON-RPC 2.0（`initialize` → `notifications/initialized` → `tools/list` → `tools/call`），复用 `mcp/mod.rs` 既有请求/响应结构（`build_initialize_request`/`build_tools_list_request`/`parse_tools_list_response` 等）与 `McpTransportConfig::Http` 传输面。客户端为 ACP 适配器（claude-code），须对齐其 streamable-HTTP 握手（`MCP-Protocol-Version` 头、`mcp-session-id`）。
- **工具 schema（5）**：
  - `list_connections` → 返回非密钥 `ConnectionSpec[]`（复用 `describe()`）。
  - `open_connection`：入参 `{ connectionId, cols?, rows? }` → `ServerConnectionStore::resolve_ssh_request(id)` → `SshSessionManager::open_remote(request)` → 返回 `sessionId` + `view`。
  - `send_input`：入参 `{ sessionId, input }` → `write(sessionId, base64)`。
  - `read_output`：入参 `{ sessionId, fromOffset? }` → 返回自 `fromOffset` 起的有界输出块（复用 `TerminalHistoryStore` 视角，不无界）；未实现缓冲时返回最近 `Output` 分块。
  - `close_connection`：入参 `{ sessionId }` → `close(sessionId)`。
- **握手/生命周期**：工具打开的会话进入全局 `SshSessionManager`/`TerminalSessionManager`，与用户 UI 共享同一会话上限（4–8）与事件 sink；应用侧「会话参考」卡实时反映。关标签/会话结束释放。
- **OTP/动态口令**：`open_connection` 驱动需口令时应用向主窗口弹 `AuthPrompt` 事件；`open_connection` 挂起 + 超时（`AUTH_PROMPT_TIMEOUT`，常量 120s）；超时未响应 → 结构化 `terminal.auth-required`。用户输入经 `provide_auth(session_id, answer)` 一次性回传，不落盘。**已落地（2026-09-09）**：`ssh_sessions.rs::keyboard_interactive` 的 `rx.await` 包上 `tokio::time::timeout(manager.auth_prompt_timeout, rx)`，超时返回新增的 `SshSessionCommandError::AuthRequired`（code `terminal.auth-required`），经既有 `fail_session` 路径清理会话（释放 slot + 移除 entry + 发 `Failed`/`Exited`）。超时值默认 `AUTH_PROMPT_TIMEOUT`（120s），`SshSessionManager::with_auth_prompt_timeout` 供测试注入短超时。回归 `ssh_sessions::tests::keyboard_interactive_prompt_times_out_as_auth_required`（内置 russh 服务器只提供 keyboard-interactive 且从不接受 → 客户端发出 `AuthPrompt` → 短超时触发 → 断言 `terminal.auth-required` 且会话已清理）。**失败码透出给 agent（已落地，2026-09-09）**：会话失败后 entry 被移除、history 被清空，agent 只见 `eof` 无法分辨失败原因。修复：`SshSessionManager` 新增有界 `recent_failures`（`VecDeque`，容量 64）保留已终止会话的失败码，`fail_session` 记录；`TerminalSessionManager::recent_failure_code`（含 `pub(crate) record_terminal_failure` 供测试注入）透出；`ConnectionOutputChunk` 新增可选 `errorCode` 字段（`skip_serializing_if=None`），`read_output` 在会话已终止时带上该码。回归 `connection_mcp::tests`（失败会话带 `terminal.auth-required` / 活跃会话无 `errorCode`）。**工具语义保持非阻塞**：`open_connection` 仍立即返回会话视图（`Connecting`/`AwaitingAuth`），由 agent 轮询 `read_output` 取终态与失败码——不引入长时阻塞的 tool call。
- **错误码**：复用 `connection.*`/`terminal.*` 语义（`terminal.session-limit`、`terminal.otp-required`、`terminal.ssh-auth-failed`、`terminal.host-key-mismatch`、`terminal.jump-not-supported`），MCP `tools/call` 返回 JSON-RPC error 且 `code` 映射到确定性错误码，不在后端拼对客文案。
- **注入点**：`build_worker_invocation`（`src/app/node_executor.rs`）与 `build_dynamic_worker_invocation`（`src/app/orchestrator.rs`）在 `mcp_servers` 计算处，按 `run.connection_specs` 非空追加该 server 配置；`prepare_acp_prompt_for_attempt` 跟随连同样注入。
- **验证边界（如实记录）**：工具 handler 与 JSON-RPC 循环可在进程内用既有 MCP 请求/响应结构写接口测试（无需 claude-code）；但 **streamable-HTTP 端到端与 claude-code 握手**依赖真实 ACP 适配器运行，留待阶段 F 客户端实操验收。实现前须在真实 agent 环境做一次 `tools/list`/`tools/call` 握手 spike，确认 claude-code 对 localhost `http` MCP 的初始化/会话/超时行为，再固化协议细节。
- **已落地骨架（2026-09-09）**：`src/app/connection_mcp.rs`——`ConnectionMcpRuntime`（5 操作 trait，`Send`）、`connection_tools_schema()`（5 工具 schema）、`handle_tools_call()`（`tools/call` 分发 + 结构化错误码）、`handle_jsonrpc()`（initialize/notifications/tools/list/tools/call 的 JSON-RPC 入口）、`connection_mcp_server_config(url)`（注入 `mcp_servers` 的 `{type:http,name,url}` 配置）、`AppConnectionMcpRuntime`（真实 runtime：`App.resolve_ssh_request` → `TerminalSessionManager.open_remote/write/close`，`read_output` 读后端历史，只允许打开 `run.connection_specs` 绑定的连接——会话级作用域，密钥由 `App` 经凭据解析、不出应用）。回归 `app::connection_mcp::tests`（14 项：协议 + runtime）。
- **已落地 HTTP 传输端点（2026-09-09）**：`src/app/connection_mcp_http.rs`——`parse_http_request`（最小 HTTP/1.1 请求解析：请求行 + headers + Content-Length body）、`respond()`（纯函数：路由 `/mcp` POST→JSON-RPC / DELETE→200 / 其他→404/405，`initialize` 成功下发 `mcp-session-id`，notification 202 Accepted 无 body）、`serve()`（`TcpListener` accept 循环，每连接独立线程）。回归 `app::connection_mcp_http::tests`（16 项，含**真实 loopback TCP + `reqwest::blocking`** 端到端集成测试）。
- **端点作用域：全局单端点 + URL 带 run 作用域（2026-09-09 定稿）**：应用起**一个**长驻 HTTP 端点（懒启动一次，端口固定/持久），URL 带查询参数标识 `taskId`/`runId`（`http://127.0.0.1:<port>/mcp?taskId=..&runId=..`）；每次 `tools/call` 按该 run 现读 `run.connection_specs`，天然支持多对话并发、互不干扰、绑定变化立即生效。落地：`ConnectionMcpRuntimeFactory::build_for_run(task_id, run_id)`（现读 run 构建 runtime）+ `connection_mcp_http::respond_scoped`（解析 query param，缺失/run 不存在返回 400 不崩溃）+ `connection_mcp_url(port, task_id, run_id)`（URL 构建）。回归覆盖 query 缺失、run 不存在、多 run 并发场景。
- **`mcp_servers` 注入（已落地，2026-09-09）**：`App` 新增 `connection_mcp_endpoint_port: Option<u16>`（`with_connection_mcp_endpoint_port`/`connection_mcp_endpoint_port()`——桌面层起监听后记录端口；`None` 表示端点未监听）。`build_worker_invocation`（node_executor.rs）与 `build_dynamic_worker_invocation`（orchestrator.rs，经 `dynamic_connection_mcp_servers` 助手）都按「该 run 绑定了连接 **且** 端点已监听」才追加 `connection_mcp_server_config(url)` 到 `mcp_servers`；任一条件不满足则不注入（不留悬空 URL）。动态路径读外层 run 失败时静默回退空列表（不让 invocation 构建失败），与既有 MCP 配置解析失败回退空列表的约定一致。回归：`node_executor::tests`（注入/端点未监听不注入/无绑定不注入）、`orchestrator::tests`（同三态 + 外层 run 缺失时容错）。
- **桌面 bootstrap 接线（已落地，2026-09-09）**：`src-tauri/src/commands.rs::spawn_connection_mcp_endpoint(app, terminal_state, history_state)`——绑定 `127.0.0.1:0`（OS 分配端口，仅 loopback，不对外暴露）、复用刚 manage 的 `TerminalSessionState`/`TerminalHistoryStore` 构建 `ConnectionMcpRuntimeFactory`、后台线程跑 `serve()`，返回实际端口。`src-tauri/src/main.rs::setup` 在 `terminal_session_state_for` 之后调用，成功则 `DesktopState::set_connection_mcp_endpoint_port(port)`；`App::app()`（`state.rs`）据此调用 `.with_connection_mcp_endpoint_port(port)`。启动失败只 `warn!` 不阻塞应用其余功能（本次进程内没有连接工具可用，等同「端点未监听」分支）。回归：`commands::tests::spawn_connection_mcp_endpoint_serves_bound_run_connections_over_real_tcp`（真实 loopback TCP 端到端：spawn → `tools/call` list_connections → 拿到绑定 spec）。当前无关闭钩子（端点与它共享状态的 TerminalSessionManager 同生命周期，跟进程走；若未来需要干净关闭，参照其他后台循环用 `DesktopLifecycleCoordinator` 持有的 `AtomicBool` 接入）。
- **未接**：OTP 的 `provide_auth` 回传与超时兜底（随阶段5 会话共享/OTP 落地）、claude-code 真实握手验收（阶段 F）。
- **集成注意（重要，需真实 agent 验收）**：注入的 server 是 `type: "http"`，会经既有 `prepare_acp_mcp_servers`（`provider/mod.rs`）按 agent 的 `mcpCapabilities.http` 过滤。若 ACP 适配器未声明 http MCP 能力，该 server 会被 **skip**（经 `acp/client.rs::record_skipped_mcp_servers` 记为诊断，不是静默丢弃）——连接工具在该 agent 上不可用。因此阶段 F 的 claude-code 握手验收必须确认其支持 `mcpCapabilities.http`；若不支持，需评估退回 stdio 传输（方案 B，成本更高）或改用其他暴露面。

#### 阶段5/7 部分：绑定写入地基 + 连接参考卡（2026-09-09）

- **背景**：阶段2 只加了 `RunState.connection_specs` 字段，`ConversationPromptInput.context_connections → run.connection_specs` 的**绑定写入从未实现**（全仓 `context_connections` 无任何读取点），因此前端卡片/标签无从取数。
- **后端绑定地基（已落地）**：`impl App` 新增 `bind_run_connection`（解析连接 → 取非密钥 `spec()` → 按稳定 id 幂等写入 `run.connection_specs` → `with_file_lock` 内读改写 + 原子写，避免并发丢失更新）、`unbind_run_connection`（幂等移除，返回是否移除过）、`run_connection_specs`（读回）。回归 `server_connections::tests`：幂等绑定+可逆解绑、未知连接 `connection.not-found`、**绑定 spec 绝不含 key path/secret**。
- **VM 投影（已落地）**：`ConversationRunVm` 新增 `connections: Vec<ConnectionSpec>`（camelCase，恰好匹配前端既有但此前未使用的 `ConnectionRefVm`），在 `conversation_run_vm` 及两个 fallback 快照中填充。
- **命令面（已落地）**：`bind_conversation_connection` / `unbind_conversation_connection`（`commands_conversation.rs`，经 `workspace_entry_for_project` 解析工作区；经 `ensure_remote_connections_enabled()` 门控，与其余远程命令一致），已在 `main.rs` 注册。
- **前端（已落地）**：`ConversationConnectionRefs`（会话顶部「连接参考」卡的 chip 行；每个 chip 自带 popover/removing 状态，避免打开 chip 或移除连接时重渲染会话正文或其他 chip；Popover 展示完整非密钥 spec；可移除）；接入 `ConversationRunHeader`（`run.connections` 非空时渲染第二行）、`ConversationRunPage`、`App.tsx`（移除后 `unbindConversationConnection` + 重新 `getConversationRun` + `applyConversationRunSnapshot(..., 'connection-unbind')`）；i18n zh/en 同步；browser preview mock 同步新增 bind/unbind + 一条绑定示例。
- **验证状态（如实记录）**：`tsc -p web/tsconfig.build.json` 0 error；`vite build` 成功；新增组件契约测试 `web/tests/conversation-connection-refs.test.tsx`（5 项：空绑定不渲染、chip 展示 name/host:port、Popover 展示完整 spec 且不含 key path、按稳定 id 移除、移除失败保留 chip 并显示错误）全绿。**浏览器实测受预览模式限制未能完成**：browser preview 下会话运行页始终停在 App 级 `BrandLoadingState`（`isConversationRunNavigationLoading || !conversationRun`），会话运行页从未挂载（新建会话与 deep link 均如此），且侧栏点击不跳转；该限制与本次改动无关（改动只加在 `ConversationRunPage` 内部与其数据字段上），故**卡片的真机/浏览器视觉验收（正常/窄/宽、明暗主题）留待可用的桌面端实测环境**。
- **未接**：composer 侧的「附连接」入口（阶段6 的发送字段/标签，用户从 UI 主动绑定）；阶段5 的「agent 打开的会话在终端面板可审阅/接管」的显式呈现（后端已共享同一 manager，会话天然出现在终端面板）。

#### 阶段6 附连接入口 + 阶段5 agent 会话呈现（2026-09-09 晚，已落地）

- **附连接入口（阶段6，已落地）**：放于会话顶部「连接参考」行（**未放 composer 输入区**——`AcpConversationComposer` 挂在约 5900 行的 `ACPChatDialog` 内，穿参侵入面大且其中一个既有失败测试就在该组件轨道；放 header 是同一用户流程的最低风险等效实现）。行尾「+ 附连接」DropdownMenu 列出未绑定的可用连接（name + host:port），点击即 `bindConversationConnection` → 重新拉取 run → `applyConversationRunSnapshot(..., 'connection-bind')`；已绑定的连接自动从菜单过滤（绑定/解绑双向自动生效，无需额外刷新逻辑）。附加中 spinner + 禁用；失败内联报错。门控：`appInfo.remoteConnectionsEnabled` 为 false 时不传 handler、入口不出现；连接列表在 header 子树内加载（唯一消费边界，不进页面级 state）。`ConversationRunSnapshotSource` 新增 `'connection-bind'`。回归：`conversation-connection-refs.test.tsx` 扩到 8 项（Radix DropdownMenu 用 pointerdown/pointerup 事件驱动，参照 `conversation-workspace-control.test.tsx` 既有模式）。
- **设计取舍说明（阶段6「发送字段」）**：设计原案提到 composer 附连接标签+发送字段；因绑定已是 **run 级持久上下文**（`build_worker_invocation`/`prepare_acp_prompt_for_attempt` 每轮从 `run.connection_specs` 现读注入），逐消息发送字段冗余——绑一次、每轮生效，语义更简单且与「会话级默认上下文」设计一致，故发送字段不再需要。
- **agent 会话呈现（阶段5，已落地）**：agent 经 MCP `open_connection` 打开的会话与用户会话共享同一 `TerminalSessionManager` + 事件 sink，但终端面板此前只在挂载时加载一次会话列表——**agent 打开的会话永远不出现在面板**（事件引用未知 id 被 map 忽略）。修复（根因级）：面板事件订阅处，事件引用未知 session id 时触发一次去重的 `loadSessions()`（`unknownSessionRefreshInflightRef` 防高频 output 事件重复刷列表），agent 会话的 tab 随即出现，状态（连接中/等待验证/已连接）随事件更新。OTP 代输闭环复用既有 `authPrompt` 内联输入 + `provideTerminalAuth`（未知会话的 AuthPrompt 会激活该 tab）。回归：`web/tests/terminal-agent-session-visibility.test.tsx`（4 项：未知会话事件刷列表、并发去重、已知会话不刷、未知等待验证会话激活 OTP 输入）。

#### 事故修复：agent「卡在连接上」——工具未注入（2026-09-15，已落地）

- **现场还原**（CC 会话 e4973238 + jiyaojun run 的 `acp.raw.jsonl`）：隐藏上下文注入**正常工作**（agent 收到 `webank (sitywts...)` spec），但 `session/new` 的 `mcpServers` 为 `[]`——agent 全场只有 Bash 工具，即兴用 `wessh`/`expect`/裸 `ssh` 自行连接，卡在动态口令提示上被用户打断多次。agent 声明了 `mcpCapabilities:{http:true}`，skip 风险排除。
- **根因（接线 bug）**：端点端口只挂在 `DesktopState::app()`，而会话运行链路用的是 `app_for_workspace()` 创建的 workspace 裸 App——端口永远 `None`，注入条件永不满足。恰好形成「看得见连接（数据注入）、用不了工具（App 字段缺失）」的割裂。
- **修复**：端口移入 `DesktopContext`（每请求 clone 的共享上下文，消除与原 `Mutex<Option<u16>>` 的双份状态），`app_for_workspace` 与 `DesktopState::app()` 都应用；端点启动**成功也打日志**（`connection MCP endpoint listening`），补观测缺口。回归：`app_for_workspace` 携带/不携带端口两态测试。
- **agent 指令契约（配套）**：`connection_hidden_sections` 内容追加工具使用契约（英文）——用 `open_connection` 打开、凭据由应用解析绝不暴露、**禁止**自行 `ssh`/`expect`/`sshpass`（说明其会卡在无法回答的交互提示上）。无此契约时即使工具在场，agent 失败一次仍会退回 Bash SSH 老路（现场观察到的放大器）。回归：注入测试断言契约存在且内容不含密钥。
- **验收手册已知点更新**：重启旧实例可能丢绑定（旧二进制无字段重写 run.json，不加兼容层）；端点监听日志是注入前提；agent 违约用 Bash SSH 属如实记录项。
- **复验发现（2026-09-15 晚）**：用户重启后日志行仍不出现，追出**两个后续 bug**：① 端点启动块在 `init_tracing` **之前**执行——成功/失败日志在 tracing 初始化前全部被静默丢弃（代码实际在跑，观测盲区）；已把启动块移到 `init_tracing` 之后。② `DesktopState::set_connection_mcp_endpoint_port` 经 `context()` 写入——而 `context()` 返回**克隆**，写入被静默丢弃，端口从未持久（端点在听但任何 App 都拿不到端口）；改为锁内直接改守卫上下文。回归：`set_connection_mcp_endpoint_port_persists_into_new_apps`（setter 后 `state.app()` 必须带端口）。
- **二次复验（2026-09-15 深夜）——真正根因：注入时机与工具集固化**。复验日志证实端点已监听（port=50404）、绑定已落盘，但 agent 仍报 `No such tool available: open_connection`（CC 帧证据 ca56f8d7 会话：agent **确实尝试了** `open_connection`——契约注入生效了）。时间线还原：会话 20:15:59 创建（当时未绑定 → 未注入），20:16:29 用户附连接，20:18 继续对话**复用旧会话**——隐藏上下文逐轮注入会更新，但 **MCP 工具集在 ACP 会话创建时固化**，中途绑定永远无法生效。修复（设计层）：**连接工具 server 无条件注入**（端点在听即注入，删掉「已绑定连接」条件）——作用域本来就由 URL 的 runId 按请求现读（未绑定时 list 返回空、open 拒绝），会话中途绑定立即生效。附带健壮性：工具参数接受 snake_case 别名（现场 agent 发的是 `connection_id`）。回归：node_executor「未绑定也注入」、orchestrator「无需读 run 即注入」、connection_mcp snake_case 别名（34 项）。
- **用户验证注意**：修复要生效需要**新建会话**——已存在的旧会话工具集是创建时固化的，须新开会话（或 rerun）。
- **三次复验（2026-09-17）——端点 400：工厂缺工作区解析 + 预附绑定竞态**。CC 会话 6bdd6b50 证据：`session/new` **已包含** gold-band-connections（无条件注入全链路打通），但模拟 claude-code 握手对运行端点直接 **HTTP 400**「failed to load run」——run 数据按工作区数据目录分片（`~/<data>/projects/<workspace-project-id>/…`），而端点工厂持有全局 App（repo_root=Gold-Band），URL 缺工作区标识导致永远找不到 run。修复：URL 增加 `projectId`（注入点取 `app.paths.project_id`），工厂增加 `workspace_paths_resolver`（桌面经 `DesktopState` 的 workspace 表解析 project→repo_root；测试直传），`respond_scoped` 强制要求 `projectId`。另修预附绑定竞态：绑定原在前端 `createConversationRun` 返回后执行，但后端 create 即后台启动首轮——绑定晚于首轮 prompt 构建（现场：首轮无隐藏块，agent 无从知晓连接）。修复：`ConversationCreateInput` 增加 `connectionId`，后端在 run 落盘后、launch 前绑定（绑定失败仅 warn 不阻塞会话）。首页预附选择经 create input 传递。回归：`connection_mcp`(34) / `connection_mcp_http`(16) / `node_executor`(16) / `orchestrator`(113) / desktop `commands`(70) / `state`(21) 全绿；URL 断言改为前缀/后缀校验（projectId 源自临时目录随环境变化）。
- **四次复验（2026-09-17 上午）——最终根因：ACP SDK 校验静默丢弃缺 `headers` 的 http 条目**。CC 会话 6bdd6b50 复验：`session/new` 已注入我们的 server，端点 400 已修，但 agent 仍报 `No such tool available`。逐层二分（真实 adapter 0.76.0 本地驱动 + 插桩）：adapter 收到的 `params.mcpServers = []`——丢失发生在 ACP SDK 的 `zNewSessionRequest` zod 校验。用该 schema 逐 transport 测试定位：**http/sse/stdio 条目缺 `headers`/`env` 等可选字段时被静默丢弃，带 `headers: []` 则存活**（内置 server 一直带 `headers: []` 所以从未出过问题）。修复一行：`connection_mcp_server_config` 显式携带 `"headers": []`。**端到端验证**：真实 adapter 0.76.0 + 真实 claude CLI + 真实 env 驱动（本地 ACP client probe），`sawToolCall = true`——agent 调用 `mcp__gold-band-connections__open_connection` 并真实发起对内网堡垒机的 SSH 连接（probe 无 UI 代输 OTP，等待符合预期）。回归：`server_config_is_http_named_and_points_at_url` 断言 `headers: []` 存在。
- **五次复验（2026-09-17）——预附绑定仍在 launch 后，被后台首轮回写冲掉**。用户实测：首页预附 → 发第一条消息 → 进入会话页时绑定丢失（chip 消失），且首轮 prompt 依旧无隐藏块。机制：绑定原写在 `launch_prepared_run_background` **之后**，而后台首轮线程启动即读 run 状态（此时 specs 未写入）并在状态迁移时回写 run.json——读前写后，绑定被冲掉。修复（设计层，born-with-specs）：`AcceptedRun::set_connection_specs` 在 launch **之前**把 spec 写进 run 状态（run 出生即带绑定），删除 launch 后的竞态绑定。同时首页预附选择改经 `ConversationCreateInput.connectionId` 传给后端创建路径（不经前端二次绑定）。
- **附带发现（非本次引入）**：desktop 测试 `commands::tests::agent_binding_usage_*`（2 项）非密封——`scheduler_db_path = core_db_path` 落在**用户级** `~/.gold-band/core.db`，生产 App 已把该库 scheduler schema 迁到 v4，测试二进制仅支持 v1 → 打开即失败；git stash 隔离验证与本次改动无关。属测试基础设施密封性问题，另案处理。
- **OTP 干预式自动展开（2026-09-17，用户反馈「不应让我自己去开面板」）**：新增 `TerminalAuthAutoOpen`（挂 `WorkspaceShell` 的 `RightWorkspaceProvider` 内）——壳层订阅 authPrompt 干预事件，触发即 `openResource({kind:'terminal', attention:true})` 自动把终端面板展开到右侧工作区并高亮；口令输入框由面板挂载后的派生态呈现。只对 authPrompt 触发，普通输出不打断用户当前工作区状态。
- **工具结果可行动化 + 面板 OTP 派生态（2026-09-17，用户实测反馈修复）**：①`open_connection` 不再立即返回 Connecting——`settle_terminal_session` 有界等待（30s，短于口令超时）直到 Connected / AwaitingAuth / 失败；失败以结构化错误码作为工具错误返回（agent 可读）。②`open_connection` 结果按状态附英文 `note`：AwaitingAuth →「提醒用户在终端面板输动态口令，然后 read_output 轮询」；Connecting →「read_output 轮询」——agent 知道下一步该做什么。③终端面板 OTP 输入框改为**派生态**：authPrompt 事件 或 活跃会话状态为 AwaitingAuth 均显示输入框——面板晚于事件挂载（agent 驱动的会话）时用户照样能输口令。i18n `terminal.authPending` zh/en。回归：connection_mcp 35 项（不可达主机结构化失败传播、settle 未知会话立即错误）。
- **堡垒机流程优化（2026-09-21，CC 会话 e4d70a73 阻断点分析）**：齐治堡垒机真实走通后发现四个 agent 侧阻断点，全部在工具契约层修复——①`send_input` **自动补回车**（`with_exec_newline`：未以行尾结尾即追加；控制字符结尾如 Ctrl+C 保持原样）——现场事故：agent 发的命令缺行尾在堡垒机里永远不执行；②`read_output` 每帧带 `status`（connecting/awaiting-auth/connected）——agent 轮询不再盲猜「等口令」还是「网络慢」；③`send_input` 命中已死亡会话时错误信息带出此前的结构化失败码（如 auth-required → agent 自动重试 open_connection 拿新 OTP）；④隐藏契约追加堡垒机指引（命令自动回车执行、齐治可能自动连默认目标或呈现菜单——读菜单发数字）。回归：`with_exec_newline`、死会话失败码透出、settle 未知会话、契约指引覆盖（connection_mcp 38 项 + provider 契约测试）。范围外观察：同一会话后期 agent 调用其他 MCP（ark 图片类）连续 1210/1234 错误，属模型/服务侧问题，与连接链路无关。
- **保活 + OTP 弹窗 + 终端选区引用（2026-09-21，用户三项优化反馈）**：
  ①**双层保活**：russh `keepalive_interval` 30s（传输层，保 TCP/NAT）+ pty 层每 60s 注入 `space+backspace`（`spawn_pty_keepalive`，任务自终止，计入堡垒机用户活动空闲计时防止自动登出；间隔经 manager 字段可注入，生产 60s/测试 300ms）。回归：echo 服务器回显 keepalive 字节测试。**真机验证项：齐治是否将 space+backspace 计入空闲计时，不计则调整注入字节。**
  ②**OTP 专用弹窗**（`TerminalOtpDialog`，App 壳层 authPrompt 事件触发，与终端面板挂载状态完全解耦）：显示连接名 + 后端原始提示 + 口令输入（提交走 `provide_terminal_auth`）；取消 = 放弃本次输入，120s 超时后 agent 自动重试 open_connection 会再触发新弹窗（不加硬上限，节奏自然缓和）。终端面板自动展开保留（可观察输出），面板内输入框保留（手动流程复用同一提交通道）。
  ③**终端选区引用**（类 CC 引用代码）：xterm `onSelectionChange` 追踪选区，选中文本后终端上方浮出「引用到对话」按钮，点击经 `terminal-quote-bus`（模块级事件桥，主窗口）跨树注入当前会话 composer 的引用列表（sourceKey `terminal:<时间戳>`）。detached 独立窗口无 composer，不渲染按钮（范围外）。回归：bus + TerminalView 按钮测试。
- **事故全链复盘（连接→对话 事故时间线）**：①工具未注入（端口接线 bug）→ ②注入条件含绑定状态（会话中途绑定失效）→ ③日志在 tracing 前丢弃（观测盲区）→ ④setter 写克隆丢失端口 → ⑤端点 400（工厂缺工作区解析）→ ⑥预附绑定竞态（create 即启动首轮）→ ⑦`headers` 缺失被 ACP SDK 校验丢弃（最终根因）。①②④⑥为 Gold Band 代码缺陷，③⑤为可观测性/契约缺陷，⑦为对 ACP SDK 行为的契约学习——全部已修复并有回归测试。
- **首页预附连接（2026-09-15 深夜补充）**：greeting 首页 composer 工具栏新增「附连接」选择（`ConversationHomeAttach`，内网渠道且有连接时渲染；选中态显示连接名、再点取消；连接列表组件自加载）。选择是 composer 草稿态（App 持单字符串 state），`createConversationRun` 成功后立即 `bindConversationConnection` 并刷新 run 快照——预附让 agent 的**第一个**会话就带有工具与隐藏上下文（配合无条件注入，彻底消除「先开会话后绑定」的时序问题）。绑定失败不阻塞会话创建（错误横幅提示，可继续对话后在运行页重附）。
- **日志路径注意（wb 渠道）**：dev:wb 实例的环境变量把 app_key/config_dir 指到 **`~/.maling/`**（`GOLD_BAND_APP_KEY=maling`、`GOLD_BAND_CONFIG_DIR_NAME=.maling`）——wb 渠道的运行日志在 `~/.maling/logs/runtime.log`，不是 `~/.gold-band/`。排查注入前提时看对文件。

#### 阶段 D：经跳板网关会话（2026-09-09 晚，已落地——标准 SSH 网关路径）

- **实现**：`SshOpenRequest.jump` 从 `Option<String>`（跳板连接 id）改为 `Option<Box<SshOpenRequest>>`（**完全解析的嵌套网关请求**，含凭据材料）——连接身份解析留在命令层，SSH 层不再需要连接实体。`App::resolve_ssh_request` 递归解析存储的 `jumpRef`（visited 集合防并发编辑下的环；**多级链（跳板的跳板）显式拒绝** `connection.invalid-input`，避免静默跳过外层跳板）。`connect_and_serve` 标准嵌套：连网关认证 → `channel_open_direct_tcpip(目标, 22)` → `Channel::into_stream()` → `russh::client::connect_stream` 嵌套第二 SSH 会话 → 目标认证 → pty/shell。`SshSessionEntry.gateway_handle` 持有网关连接至会话生命周期（drop 即断转发通道）。移除阶段 C 的 `terminal.jump-not-supported` 拒绝路径（连带前端死映射/死文案删除，破坏式更新），新增 `terminal.jump-forward-failed`（网关认证成功但 direct-tcpip 转发/嵌套握手失败）与 `terminal.auth-required` 的前端文案（zh/en）。
- **跳板认证**：网关与目标分别走各自的 auth 材料（口令/密钥/键盘交互 OTP 均可用）；任一跳需要动态口令都会经同一 `AuthPrompt` 事件弹给用户。
- **测试（进程内端到端，无需真实网关账号）**：`spawn_jump_gateway` 起一个进程内 SSH 网关（接受口令认证 + 把每个 direct-tcpip 请求桥接到指定 TCP 目标，双向 pump），目标机复用既有 EchoServer：`opens_remote_session_through_a_jump_gateway`（嵌套登录 → 目标 shell 输出穿透网关到达 sink → 输入回显 → 关闭清理）、`jump_forward_to_an_unreachable_target_fails_structured`（网关活着但目标不可达 → 结构化 `terminal.jump-forward-failed`）、`resolve_ssh_request_rejects_a_multi_level_jump_chain`（多级链拒绝）。
- **真机待验（阶段 F）**：WindTerm 二次开发网关是否为标准 SSH 协议（spike 结论待补）；内部真实网关账号下 direct-tcpip 是否放行。若网关非标，按既定风险对冲改接公司 CLI 适配器（传输适配器抽象位置在 `connect_and_serve` 的嵌套分支）。
#### OTP 输入路由与工具失联自愈（2026-10-08，现场事故修复）

- **事故一（口令无输入框）**：口令等待是后端 120s 的持续状态（`AwaitingAuth`），但 App 壳层弹窗只由瞬时 `authPrompt` 事件触发——webview 重载、事件订阅异步就绪前、或错过事件后整条链路无兜底，用户停在会话页时终端面板的派生态也帮不上（面板未挂载/非激活 tab），会话只能干等 120s 超时。
- **事故二（工具幻觉 + MCP 断链）**：CC 会话内建 `/model`（composer 斜杠菜单转发进会话的 local 命令）切换模型后，agent 侧工具集重建，ACP `session/new` 注入的 `gold-band-connections` 工具从可用列表消失；Gold Band 复用指纹（providerId+cwd+mcpServers）感知不到 agent 侧失联，后续每轮继续复用该会话 → 失联永久化 → 模型在工具列表里找不到目标工具，改抓 context7 幻觉调用（8 次连续）。

**OTP 输入路由（按发起方分流，状态派生兜底）**：
- **来源标记**：连接 MCP 工具路径打开会话时经 `SshOpenRequest::with_origin` 打上 `ConnectionRunLocator{projectId,taskId,runId}`；`TerminalSessionView.origin` 与 `AuthPrompt` 事件均带出。手动（终端面板/连接页）打开为 `None`。
- **会话内输入卡片（主路径，用户预期）**：发起对话在屏幕上时，口令输入框由该对话的 `ACPChatDialog` 在 composer 上方渲染（`ConversationOtpCard`，AskUserQuestion 式、非阻塞），提交走既有 `provide_terminal_auth`，**口令不经过模型**。工具契约与 `open_connection` 的 note 已同步改为「输入框已由应用给出，不要向用户索要口令」。
- **壳层弹窗（兜底）**：手动连接、或发起对话不在屏幕上时，保留 `TerminalOtpDialog`。
- **状态派生恢复（修事故一）**：口令请求不再只活在组件态——模块级 `terminal-otp-bus` store 承接，App 壳层是 terminal 事件唯一订阅者并写入 store；挂载（webview 重载）与切入对话 run 时用 `listTerminalSessions()` 的 AwaitingAuth 视图 reconcile 恢复；事件只是加速器。用户主动关闭的会话不再弹出，新的 `authPrompt` 重新给出；会话终结（failed/exited/离开 AwaitingAuth）自动消失。
- **可观测性**：`keyboard_interactive` 口令等待/应答/超时打结构化日志（`gold_band::app::terminal`，含 origin）——此前该路径零日志，「没弹框」不可审计。

**工具保真与失联自愈（修事故二）**：
- **防触发**：composer 提交 `/model`（`isSessionMutatingLocalSlashCommand`）不再转发原始 prompt——带值时按会话配置的可用模型解析并走 `setAcpSessionModel`（`session/set_config_option`，同一会话内安全，不重建工具集），未匹配给出结构化提示；无值提示用法。
- **失联检测**：连接工具契约（`src/prompts/{zh-CN,en}/connections/tool-usage.md`，从 provider/mod.rs 硬编码迁入并双语维护）要求 agent 在工具不在可用列表时**禁止改调其他工具/编造调用**，输出稳定标记 `[gold-band:connection-tools-missing]` 结束回合。
- **自动重注册**：`run_prompt` 回合末扫描可见文本命中标记（`plan_connection_tools_reload`：仅正常完成且无终态失败）→ 结构化诊断（`connection_tools_missing`）→ `AcpSessionRuntimeRegistry::detach_for_reload` 使 attached session 失效 → 下一轮走 resume/new 并带完整 mcpServers，工具自动回来。误报代价仅为一次会话重建，可接受。
- **可观测性**：`session/resume`/`session/load` 补 perf 日志（含 `mcp_server_count`）——此前只有 `session/new` 有，resume 路径的注入完全不可审计。
- **回归**：Rust——`connection_tools_reload_requires_completed_turn_with_marker`（判定契约）、`connection_tool_usage_instructions_cover_bastion_and_execution_semantics`（双语契约含标记）、ssh origin 事件断言、connection_mcp factory origin 断言；Web——`terminal-otp-bus.test`（错过事件 reconcile 恢复 / 关闭后不弹 / 新事件重出 / 终结消失）、`conversation-otp-card.test`（卡片 DOM 验收：可见/提交/失败保留/取消）、`slash-command-local-model.test`（/model 识别与解析）。
- **真机待验**：wb 渠道 + webank 连接走一遍会话驱动连接（agent `open_connection` → 会话内卡片输 OTP → connected → 驱动堡垒机看日志）；在会话中 `/model <id>` 确认不再转发且模型实际切换。

#### 工具失联恢复闭环补全（2026-10-12，现场事故 d596d1c9 复盘）

- **事故**：用户发错指令（`wessh aide-core`）后在会话中执行 `/model sonnet`（CC 内建 local 命令），随后纠正指令时 agent 工具列表里 `gold-band-connections` 已消失（误触 context7 两次被拒），agent 按契约输出失联标记停止——**恢复闭环断在最后一步**：runtime 已检测标记并 detach（日志 `connection tools missing detected; detaching`），但"重发请求"留给了用户，且用户未重发，连接操作终止。
- **根因一（拦截穿透）**：`/model` 拦截此前依赖动态 agent 命令目录（`parseCommittedSlashCommand` 需在 catalog 中命中 `model`），目录未加载/未收录时 `/model sonnet` 原样转发。修复：`parseModelSlashInput` 固定模式 `/^\/model(?:\s|$)/i` 直接匹配原始输入（CC 内建命令是固定知识，不依赖运行时目录），参数路由到产品模型切换，未匹配给结构化提示。
- **根因二（恢复死路）**：恢复态未呈现、重试无入口。修复：`deriveConnectionToolsRecovery` 从时间线纯派生——最新一条消息是含 `[gold-band:connection-tools-missing]` 的 agent 消息（其后无新用户轮次）时，composer 上方显示恢复横幅 + **「重试上一条消息」**按钮（重发最近一条用户消息，走既有 submitPrompt）；用户重发后横幅自动消失。后端无需改动（标记本就在可见消息流中）。
- **验证**：Web——`slash-command-local-model.test`（固定模式：带参/裸命令/前置空白/大小写/lookalike 不误伤）、`connection-tools-recovery.test`（标记检测、非消息项不干扰、重发后清除、历史陈旧标记不触发、无用户消息时重试文本为空）。
- **恢复死循环根因补全（2026-10-12 下午，事故 cb3ff6e1）**：重试按钮触发了、runtime 检测/detach 也生效了，但工具仍未回来——根因在适配器：`getOrCreateSession` 对 resume 有**活会话快速路径**，会话定义指纹（含 mcpServers）一致时直接复用现有 Query 进程、不重建；`/model` 在进程内销毁工具集后，参数不变的任何重发都命中快速路径，工具永远回不来（日志实证：重试轮 `outcome=reused`、无重建、再次失联标记；`mcp_server_count=1` 只是发送侧计数）。修复：失联标记改为**递增重注册 epoch**（attempt 目录 `connection-tools-reload.json`，原子写），下一轮 setup_session 把 `?reload=<epoch>` 追加进连接 MCP URL——适配器侧指纹必然变化 → teardown 活会话并带 resume 重建 → 工具重新注册且对话历史保留；epoch 稳定期内参数不变，不产生多余重建。同时修正上一轮的 detach 时机缺陷：`release_managed_session` 会在回合末把会话原样插回注册表，回合内的 detach 被回插抵消（现场日志证实重试轮复用了同一活会话），故检测点不再 detach、只 bump epoch，由下一轮的指纹失配走正常 reload 路径。回归：epoch 往返/递增、URL 参数插入/替换/保序、仅改连接 server、指纹随 reload 参数变化（acp::client 135 项）。
- **待真机验证（更新）**：重试后 runtime 日志应出现 `ACP session restore completed ... mcp_server_count≥1`，且 agent 工具恢复（可直接 `read_output` 原 sessionId 继续）；注意 `/model` 拦截依赖前端 bundle 生效，若 dev webview 未热更新请重启 dev 实例后再验证。
