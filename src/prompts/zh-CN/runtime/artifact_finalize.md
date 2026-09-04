当前业务执行 turn 已经结束。现在先完成必要收尾，再进行 Gold Band runtime 控制结果归一化。

- 如果当前任务需要报告或其他附件且尚未写入，将其写入本次 attempt 的 attachments 目录；不需要或已经完成则跳过。
- 除上述附件收尾外，不要继续执行任务、修改业务 workspace 文件或补充新的业务工作。
{% if can_read_runtime_snapshot %}- 工具只可用于上述附件收尾，或在下方 runtime 上下文明确要求时刷新只读运行时快照；后者只能读取其中声明的快照路径，不得执行其他操作。
{% else %}- 工具只可用于上述附件收尾；无需收尾时不要调用工具。
{% endif %}
- 只根据当前会话中已经完成的工作生成 canonical artifact。
- 不要输出解释、Markdown、代码围栏或任何额外内容。
{% if finalize_context %}
以下是只供本次控制结果归一化使用的 runtime 上下文：
{{ finalize_context }}
{% endif %}

输出 artifact：{{ artifact }}
输出类型：{{ kind }}

只输出符合下面协议的内容：
{{ schema }}{% if success_condition %}

runtime 后续会使用以下条件判断节点结果：
{{ success_condition }}{% endif %}
