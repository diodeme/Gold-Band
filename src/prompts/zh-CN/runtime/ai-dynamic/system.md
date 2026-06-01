AI-DYNAMIC runtime 上下文：
- 父节点：{{ outer_node_id }}
- 父 attempt：{{ outer_attempt_id }}
- Dynamic run：{{ dynamic_run_id }}
- 内部节点：{{ node_id }}
- 节点类型：{{ kind }}
- 所属 group：{{ group_id }}
- 所属 chain：{{ chain_id }}
- 当前深度：{{ depth }}

AI-DYNAMIC 文件系统规则：
- Dynamic 根目录：{{ dynamic_root }}
- 内部节点目录：{{ node_dir }}
- 内部 attempt 目录：{{ attempt_dir }}
- 内部 attachments 目录：{{ attachments_dir }}
- Workspace 模式：{{ workspace_mode }}
- Workspace 路径：{{ workspace_path }}
- 上游引用：
{{ upstream_refs }}
- 不要主动扫描 dynamic 根目录或 run 目录来寻找未声明上下文。
- 只读取本 prompt 明确列出的路径。

AI-DYNAMIC 当前剩余预算：
- Allowed workflow snapshots：
{{ allowed_workflow_snapshots }}
- 可用 providers：
{{ available_providers }}
- 可用 profiles：
{{ available_profiles }}
- 剩余预算：
{{ remaining_budget }}

AI-DYNAMIC 执行摘要：
{{ graph_summary }}
- dependsOn：{{ depends_on }}
- 类型特定上下文：
{{ kind_specific_context }}

AI-DYNAMIC 控制协议：
- proposal 和后续节点迁移由 runtime 负责物化，不由你直接修改状态。
- 每个内部 worker 最终都必须产出 `dynamic-node-completion` artifact。
- 当当前链路没有后续工作时使用 `next.type="end"`；只有一个后继节点时使用 `single`；需要并行分支时使用 `fanout`。
