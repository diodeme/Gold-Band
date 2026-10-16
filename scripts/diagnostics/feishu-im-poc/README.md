# 飞书 IM PoC

验证两件事：扫码创建飞书机器人；飞书端审批卡片（Permission / ManualCheck / Elicitation / 通知）的完整交互链路。
独立诊断脚本，不属于产品代码，不参与构建。

## 运行

```powershell
cd scripts/diagnostics/feishu-im-poc
npm install
npm run register          # 打开 http://127.0.0.1:47831 扫码创建应用，并自检凭证/机器人/长连接/权限
npm run run -- --minutes 30   # 建立长连接并向扫码用户下发 12 张审批测试卡片（suite=approval）
npm run run -- --suite custom # 下发 5 张自定义回答测试卡片：单选/多选+“其他”、自由文本、混合 5 题、拒绝回答
npm run report            # 输出 Markdown 报告
npm test                  # core.mjs 单元测试
```

在单聊回复“重发”可重新下发当前 suite 的全部卡片。凭证也可通过 `FEISHU_POC_APP_ID` / `FEISHU_POC_APP_SECRET` / `FEISHU_POC_OPEN_ID` / `FEISHU_POC_BRAND` 提供。

## 文件

- `core.mjs`：纯逻辑（场景、Card JSON 2.0 构建、回调解析、请求收敛、事件去重、脱敏、报告）。
- `core.test.mjs`：`node --test` 单元测试。
- `poc.mjs`：CLI（register / run / report），基于 `@larksuiteoapi/node-sdk`。
- `.poc-state/`：运行产物，已被 `.gitignore` 排除；`credentials.json` 含 App Secret，禁止外传。

## 验证结论（2026-10-12，飞书，SDK 1.74.0）

| 能力 | 结论 |
| --- | --- |
| 扫码创建 | 通过。`registerApp` 设备码流程约 5 分钟（含人工确认）；确认页只选头像，不展示预置权限；免审批自动发布 |
| 预置权限 | 通过。`addons.scopes.tenant` 三项全部授予；`im:message:update` 名称有效 |
| 预置事件/回调 | 通过。`im.message.receive_v1` 与 `card.action.trigger` 自动订阅，默认长连接，无需后台配置 |
| 卡片回调 | 通过。经长连接以 `event` 帧送达，SDK `WSClient` 可直接处理；端到端约 90–100ms |
| 回调载荷 | `action.value` 原样回传；表单提交带 `action.form_value` 与 `action.name`；`operator.open_id` 可用于绑定用户校验 |
| 单选 / 下拉 20 项 | 通过。`form_value.q0` 为字符串；必填由客户端拦截 |
| 多选 | 通过。选中时为字符串数组；**空选时 `form_value` 字段整体缺失**，解析时须视为显式空选 |
| 多问题表单 | 通过。未答完客户端拦截；三项答案按字段名返回 |
| 回调内返回卡片 | 通过。返回 `card: {type:'raw'}` 即原地更新为终态，按钮不可再点 |
| 桌面端先处理 | 通过。`im.v1.message.patch` 异步更新约 0.7s；已处理卡片再次点击可收敛为桌面结果 |
| 回调超时（>3s） | 客户端提示超时，不重投同一 `event_id`；回调返回值被丢弃，需异步 PATCH 补终态（已验证可行） |
| 单聊消息 | 通过。`im.message.receive_v1` 实时送达，机器人回复迅速 |
| 群聊 @ | 未申请群消息权限时不会收到群消息事件，天然拒绝 |
| 其他（自行填写） | 通过。选项与同表单 `input` 一次提交：`{"q0":"other","q0_other":"…"}`；多选同理 `{"q0":["unit","other"],"q0_other":"…"}` |
| 输入框回传形态 | 空输入回传 `""`（不同于空多选整体缺失）；未选“其他”时输入框的值**仍会回传**，服务端须忽略 |
| 条件校验 | 表单内无法按选项条件显示/必填输入框；“选其他必须填写”由服务端校验，返回仅含 toast 的响应时卡片保持可编辑，可修正后重提（已验证） |
| 自由文本 | 通过。`required` 由客户端拦截；多行输入可换行；选填不填可提交。`max_length` 不能作为唯一约束，服务端须再校验长度 |
| 混合 5 题 | 通过。单选/多选/文本/其他混排，一次提交返回全部字段 |
| 表单外拒绝回答 | 通过。表单外按钮不触发表单必填校验，回调无 `form_value`，可直接收敛为“拒绝回答” |

## 清理

1. 删除 `.poc-state/`（含 App Secret）。
2. 在飞书开发者后台删除应用 “Gold Band PoC”。
