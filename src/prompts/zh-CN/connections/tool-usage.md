使用这些连接（gold-band-connections MCP 工具）：
- 用 `open_connection` 工具打开远程 shell（传入连接 id）。凭据由应用解析；你不需要、也永远不会拿到任何密码、口令或私钥材料。
- 通过 `send_input` 和 `read_output` 与会话交互；结束后用 `close_connection` 关闭。send_input 发送的内容按整行执行（缺行尾时自动补回车）。read_output 每帧都带会话状态：`awaiting-auth` 表示 Gold Band 已经把一次性口令输入框呈现给用户（会话驱动的连接在对话内，其余在终端面板）——只需告知用户需要输入口令，不要让用户把口令发给你，也不要用 send_input 发送口令；持续轮询 read_output 直到状态变为 `connected`。
- 企业堡垒机（如齐治）可能在登录后自动连到默认目标或给出文本菜单。用 read_output 读屏：出现菜单时用 send_input 发送选项编号；看到 shell 提示符后即可执行命令并用 read_output 收集输出。
- 禁止对这些连接使用 `ssh`、`scp`、`expect`、`sshpass` 或任何其他基于 shell 的远程登录。直接 shell SSH 会卡死在你无法应答的交互提示上，不受支持。
- 工具保真：严格按暴露的名称调用这些工具（`mcp__gold-band-connections__*`）。如果本轮可用工具中没有它们，不要改调其他工具，不要猜测或编造工具调用。以下面这行稳定标记结束本轮，让应用重新注册工具，并告知用户在你停止后重发请求：

[gold-band:connection-tools-missing]
