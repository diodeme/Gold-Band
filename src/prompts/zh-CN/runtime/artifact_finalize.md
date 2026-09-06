系统收到了你上一轮执行结束的信号。请根据你当前的执行意图，决定是否结束本节点。

- 如果你没有打算结束本节点，请直接继续执行，沿用当前任务范围、工作区和工具权限；无需输出状态标签，也不要为了回应本提示而提前交接。继续执行后，当你认为本节点已经结束时，再输出下方 artifact。
- 如果你认为本节点已经结束，请根据已完成的工作输出下方 artifact。无需重新核对任务目标或验收要求，也不要因为本提示新增业务工作。
- 输出 artifact 前，如果当前任务需要报告或其他附件且尚未写入，将其写入本次 attempt 的 attachments 目录；不需要或已经完成则跳过。
{% if can_read_runtime_snapshot %}- 生成 artifact 时，如下方 runtime 上下文明确要求刷新只读运行时快照，只能读取其中声明的快照路径。
{% endif %}- 继续执行期间可以正常回复和使用工具；只有最终输出 artifact 时，不要附加解释、Markdown 或代码围栏。
{% if finalize_context %}
以下是只供本次控制结果归一化使用的 runtime 上下文：
{{ finalize_context }}
{% endif %}

输出 artifact：{{ artifact }}
输出类型：{{ kind }}

仅在你认为本节点已经结束、决定输出 artifact 时，遵守下面协议：
{{ schema }}{% if success_condition %}

runtime 后续会使用以下条件判断节点结果：
{{ success_condition }}{% endif %}
