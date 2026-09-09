上一轮 `dynamic-node-completion` proposal 尚未接受，请处理下列校验项或提醒后重新输出。

你必须修复最终的 `dynamic-node-completion` 输出，使其满足下面这些 runtime 约束。
只修复协议校验错误，不重新执行任务；后继任务仍须符合范围契约，越界项只删除或收窄。
{% if fanout_workspace_dirty %}
本次fanout即将从HEAD开始创建worktree，检测到源工作区仍有未提交代码，故提醒：
- 分叉源工作区：{{ fanout_workspace_path }}。请检查是否有本次任务产生、且后续分支需要的业务改动尚未提交；如有，审阅后按具体路径提交，可使用 Conventional Commits。
- 如无需要提交的改动，不做任何 Git 操作，直接重新输出 artifact。不要求工作区干净或产生新 commit；之后不会再次因脏文件阻止 fanout。
- 不要为了本提示清理工作区、stash 无关内容、移动其他 worktree、盲目 `git add -A` 或改变忽略规则。保留无关内容，不丢弃无法安全处理的改动。
- 重新输出的 artifact 仍须满足其他协议校验，不要修改 schema 或添加 workspace/branch 字段。
{% endif %}
不要输出解释、Markdown、代码围栏或任何额外内容，只输出修复后的 `dynamic-node-completion` 内容。

{% if has_coordination_snapshot %}最新协调快照：
- 只读快照：{{ coordination_snapshot_path }}
- 修复并输出 `next.type="single"` 或 `next.type="fanout"` 前读取最新协调快照；只能读取，不要修改该文件。
{% endif %}

校验错误：
{{ validation_errors }}

当前合法值参考：
{{ repair_reference }}

当前剩余预算：
{{ remaining_budget }}
