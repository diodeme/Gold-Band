# 桌面端头像系统

## 1. 目标

为个人消息与 Agent 消息提供统一、用户级持久化的头像能力。默认状态不设置图片，继续显示 Bot / User 图标；设置后，设置页与所有 ACP 会话详情消费同一份头像偏好。

## 2. 领域模型

头像数据属于桌面个性化领域，不属于 task、run、round 或 ACP session 生命周期。

```text
AvatarPreferences
├── agent: AvatarProfile
└── user: AvatarProfile

AvatarProfile
├── shape: circle | square
├── selectedAvatarId: string | null
└── recentAvatars: AvatarRecord[0..10]

AvatarRecord
├── id
├── fileName
├── mimeType
└── createdAt
```

- Agent 与个人头像分别管理当前头像、形状和最近列表。
- 默认形状为圆形，默认 `selectedAvatarId = null`。
- 最近头像按最近使用顺序排列，最多 10 个；选择历史头像时将其移动到首位。
- 图片二进制与元数据分离：图片位于用户级 `desktop/avatars/`，元数据位于 `desktop/avatar-settings.json`。

## 3. 接口

- `save_desktop_avatar(input)`：保存裁剪后的图片、更新形状、选中项与最近列表。
- `select_recent_desktop_avatar(kind, avatarId)`：切换最近头像并更新最近使用顺序。
- `save_desktop_avatar_shape(kind, shape)`：只更新头像框形状。
- `PreferencesVm.avatars`：启动与偏好保存后返回完整头像 ViewModel；图片以 data URL 提供给 WebView，前端不直接访问本地文件路径。

错误使用结构化错误码：`avatar.unsupported-image-type`、`avatar.invalid-image-data`、`avatar.image-too-large`、`avatar.recent-not-found`、`avatar.load-failed`、`avatar.save-failed`。后端不返回对客文案，前端按语言映射。

## 4. 上传与裁剪

- 支持 PNG、JPEG、WebP 原图，原图上限 10 MB。
- 前端复用浏览器原生文件选择器、`react-easy-crop` 和 shadcn/ui Dialog、DropdownMenu、Slider、Avatar。
- 裁剪比例固定 1:1；圆形头像显示圆形取景框，方形头像显示方形取景框与网格。
- 裁剪结果统一为 320×320 WebP，质量 0.9；后端限制处理后文件不超过 1 MB并校验 MIME 文件签名。
- 头像框形状是展示偏好，不重复生成两份图片；同一裁剪结果可在圆形与方形之间即时切换。

## 5. 设置页交互

- 设置页 tab 为“通用 / 个性化 / 高级”。
- 个性化顺序为“外观 / 字体 / 头像”，头像作为低频设置放在主题和字体之后。
- 头像区在宽内容区并列展示 Agent 头像和个人头像；单项采用 48px 头像、名称、说明与圆形/方形操作组成的紧凑横向行，窄内容区允许操作自然换行，不再单独展示“头像框”标签。
- 点击头像打开下拉菜单；菜单左边缘与头像左边缘对齐，优先向右展开，并保留视口边缘碰撞避让。
- 下拉菜单先展示最多 10 个最近头像缩略图，再展示“上传头像”。没有历史时展示明确空状态。
- 头像形状使用圆形/方形双按钮选择；保存期间禁止重复提交。

## 6. 会话展示

- 真实 Agent 文本消息显示 Agent 头像，真实用户消息显示个人头像。
- 会话头像尺寸为 36px，时间位于头像下方；未设置图片时显示 Bot / User fallback。
- 思考、工具、计划、权限等结构化行不显示头像，只保留 36px 横向占位，避免重复头像干扰信息层级。
- 圆形/方形样式由共享 `AvatarDisplay` 统一处理，设置页和会话不得自行拼接另一套头像样式。

## 7. 验收

- 默认无图片且 Agent / 个人状态相互独立。
- 上传后可裁剪、保存并立即预览；重启后仍可恢复。
- 最近列表最多 10 个，可重新选择历史头像。
- 圆形与方形在设置页及会话详情保持一致。
- 头像设置位于外观和字体之后，两个头像项保持紧凑且在窄宽度下不溢出。
- 结构化 ACP 行无头像，文本消息头像尺寸与对齐正确。
- Rust 接口测试覆盖持久化、最近上限、历史选择、形状和非法图片；Web 测试覆盖前端模型与浏览器 API；前端生产构建和 deep link 页面验证通过。
