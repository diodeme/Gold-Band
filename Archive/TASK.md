> 产品名：Gold Band（中文名：金箍）

## 背景：
当前使用claude code或者codex等代码agent工具时，已逐渐形成specdd的共识，即设计 --> 规划 --> 开发--> 编译 --> 测试 的工作流，其中编译或测试失败会回滚到开发，形成这样的循环。具体的工作流实现在claude code中体现为，一个skill对工作流进行编排，每一个步骤由预先设置好的subagent实现，主会话只做编排能力。同时还存在一种思路，使用ralph loop解决agent单次执行没有彻底解决完任务的问题，ralph的核心逻辑就是通过脚本调起claude code，然后给claude code任务，然后判断claude code是否输出了约定，如果输出约定则终止循环，没有输出约定则重新调起一个新的claude code，继续把任务给他实现，直到claude code输出了约定。

## 痛点：
1.工作流是由agent调用模型实现的，模型根据当前上下文决定下一步调用哪一个subagent，上下文太长会导致主会话产生注意力漂移，任务分发会出现问题
2.根据claude code官方文档，subagent不支持自动发现skill，只支持预设skill，所以如果你将一个subagent作为开源项目分享出去，是无法开箱即用地使用用户的skill生态的。
3.ralph loop在agent工作结束时判断agent是否输出了约定词，但是无法避免模型撒谎的问题，事实证明，agent的一次完成率很低，这里就包括agent实际上没有完全能完成用户需求，但是依然声称自己完成的情况，所以ralph loop最好是需要使用另一个agent对当前的agent执行结果来进行交叉验证

## 当前痛点的一些已知解决方案：
基于agent的工作流项目：
- https://github.com/stellarlinkco/myclaude
- https://github.com/ruvnet/ruflo
- https://github.com/AvivK5498/The-Claude-Protocol
- https://github.com/vxcozy/workflow-orchestration
基于系统的工作流框架：
- https://github.com/langchain-ai/langgraph
- https://github.com/pydantic/pydantic-ai
- https://github.com/agno-agi/agno

针对agent的工作流项目还是专注在使用多agent、prompt设计等方法减弱自回归漂移问题，但无法完全杜绝，好处在于可以原生集成在claude code等项目，没有编程压力
针对系统的工作流框架可以通过图来编排工作流，可以避免自回归漂移问题，但是太重，无法原生集成cc等工具，需要一定编程门槛


## 想法：
核心诉求：使用一种简易的DSL来编排工作流，目标是介入agent增强和系统框架之间的生态位，特性是可以做到程序级别的工作流稳定性，又足够轻量级，可以原生集成在claude code等项目，没有编程压力

设计 --> 规划 /--> 开发 --> 编译 --> 测试 --> --> 清理 --> 验证

## 相关链接：
1.[claude agent team](../claude-agent-team.md): 关于claude code的agent team功能的文档
2.[claude-hooks](../claude-hooks.md): 关于claude code的hooks功能的文档
3.[claude-skills](../claude-skills.md): 关于claude code的skills功能的文档
4.[claude-scheduler](../claude-scheduler.md): 关于claude code的scheduler功能的文档
5.[claude-sdk](../claude-sdk.md): 关于claude code的sdk功能的文档
6.[claude-subagents](../claude-subagents.md): 关于claude code的subagents功能的文档
7.[ralph.sh](../ralph.sh): 关于ralph loop的实现脚本