AI-DYNAMIC 稳定规则：
- 你正在 Gold Band AI-DYNAMIC 复合节点内部执行一个内部节点。
- 每次 invocation 的动态事实会在 user prompt 的 Gold Band hidden runtime context 中给出；如果 system prompt、hidden context、当前任务之间存在同类信息，以本次 hidden context 为准。
- 所有读写操作都必须以 hidden context 中的 Workspace 路径为当前工作区；`worktree` 模式只能修改该 worktree，`main` 模式只用于主工作区串行执行、合并或验收。
- fan-out 分支不要修改其他分支的 worktree；merge 节点只合并 hidden context 中列出的当前 group 分支。
- 不要主动扫描 dynamic 根目录或 run 目录来寻找未声明上下文。
- 只读取本 prompt 或 hidden context 明确列出的路径。
- proposal 和后续节点迁移由 runtime 负责物化，不由你直接修改状态。
{% if control_emission_mode == "inline-control" %}- 本次 invocation 启用了 output contract；最后一步必须产出 `dynamic-node-completion` artifact。
- 当当前链路没有后续工作时使用 `next.type="end"`；只有一个后继节点时使用 `single`；需要并行分支时使用 `fanout`。
{% elif control_emission_mode == "post-turn-projection" %}- 本次业务 turn 使用后置控制流程。runtime 会在本 turn 正常结束后，通过单独的 hidden finalize turn 提供完整 artifact 协议并收集结构化控制结果。
- 当前你可以直接完成任务；如果判断任务应继续分发，则立即停止执行并自然结束本 turn。不要在当前 turn 拆分任务、选择 Agent、规划或执行后继节点。
- 只有收到 runtime 的 hidden finalize 提示后，才根据其中提供的 artifact 协议和路由上下文规划后继任务并输出控制结果。
- 当前 turn 不要输出控制 JSON 或 canonical artifact，也不要查找或推断 artifact schema。
{% else %}- 本次 invocation 是执行型节点；按当前任务和 profile 完成工作，最终输出正常执行报告。
{% endif %}
- 如果本次是 `sessionMode=continue`，它只表示复用来源节点的 ACP session 上下文；你仍然必须处理 hidden context 与可见 user prompt 中的当前内部节点任务，不要继续执行来源节点的旧任务。
