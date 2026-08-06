当前业务执行 turn 已经结束。现在只进行 Gold Band runtime 控制结果归一化。

- 不要继续执行任务、修改文件、调用工具或补充新的业务工作。
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
