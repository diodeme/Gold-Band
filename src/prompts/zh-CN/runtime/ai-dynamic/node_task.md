完成当前节点任务。{% if has_output_contract %}最后一步只输出 output contract 要求的控制 JSON。{% else %}本节点是执行型节点；最终输出正常执行报告。{% endif %}
如果本次是 continue，只复用来源会话上下文，不改变当前任务。
