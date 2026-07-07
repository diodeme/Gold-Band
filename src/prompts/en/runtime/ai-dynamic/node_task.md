Complete the current node task. {% if has_output_contract %}As the final step, output only the control JSON required by the output contract.{% else %}This is an execution-only node; finish with a normal execution report.{% endif %}
If this invocation is a continue, only reuse the source session context; do not change the current task.
