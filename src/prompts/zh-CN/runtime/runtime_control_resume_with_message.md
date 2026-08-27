{{ user_message }}
<hidden data-gold-band-hidden="true" show="false" title="Gold Band runtime control">
{% if artifact_emission_mode == "post-turn-projection" %}请先完整执行本消息中的用户指令，然后继续完成你之前的任务。本 turn 不适用此前的 artifact 输出约束，也不要输出 artifact；完成后再由 Runtime 在后续独立 turn 中完成结果归一化。{% elif artifact_emission_mode == "inline-control" %}请先完整执行本消息中的用户指令，然后继续完成你之前的任务，完成后再按当前输出契约输出 artifact。{% else %}请先完整执行本消息中的用户指令，然后继续完成你之前的任务。{% endif %}
</hidden>
