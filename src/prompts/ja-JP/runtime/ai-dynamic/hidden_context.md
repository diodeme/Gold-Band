# 本呼び出しの AI-DYNAMIC runtime コンテキスト

## 現在の dynamic ノード
- Parent node: {{ outer_node_id }}
- Parent attempt: {{ outer_attempt_id }}
- Dynamic run: {{ dynamic_run_id }}
- Internal node: {{ node_id }}
- Title: {{ title }}
- Kind: {{ kind }}
- Group: {{ group_id }}
- Chain: {{ chain_id }}
- Depth: {{ depth }}

## Runtime 位置
- Dynamic root: {{ dynamic_root }}
- Internal node（Dynamic root からの相対）: {{ node_dir }}
- Current attempt（内部ノードからの相対）: {{ attempt_dir }}
- Attachments（現在 attempt からの相対）: {{ attachments_dir }}
- Workspace ID: {{ workspace_id }}
- Workspace path: {{ workspace_path }}
- Workspace capability:
{{ workspace_capability }}

{% if has_new_round_trigger %}
## `$new-round` トリガーフィードバック
{{ new_round_trigger }}
- これは現在の新 Round を開いた失敗ノード出力です。本 Round の内部タスクを計画する前に、その失敗理由と未完了作業を理解してください。元の要件をそのまま繰り返さないでください。
- artifact プレビューは切り詰められている場合があります。完全な詳細が必要な場合は、明示的に列挙された artifact または添付を読み取ってください。
{% endif %}

{% if has_coordination_snapshot %}
## Runtime coordination スナップショット
- 読み取り専用スナップショット（Dynamic root からの相対）: {{ coordination_snapshot_path }}
- Runtime は canonical dynamic graph からこのファイルを導出し、唯一の writer です。変更してはならない。
- 本タスクを開始または継続する前に最新スナップショットを読み取る：各 `workstreams[]` の goal、TODO ステータス、親関係、steps を使用して他サブタスクを理解し、`groups[]` のネストと phase で重複または競合作業を避ける。
- `next.type="single"` または `next.type="fanout"` を出力する前に同じパスを再度読み取り、最新状態から後続を計画する。
{% endif %}

{% if has_direct_predecessors %}
## Direct predecessors
{{ direct_predecessors }}
{% endif %}

{% if has_active_group %}
## Active group
{{ active_group }}
{% endif %}

{% if has_inherited_groups %}
## Inherited group context
{{ inherited_groups }}
{% endif %}

{% if has_siblings %}
## Parallel siblings
{{ siblings }}
{% endif %}

{% if has_available_attachments %}
## Available attachments
- 添付パスのみ列挙。添付内容は読み取らずインライン化しない。通常エントリの完全パスは `Dynamic root` と連続パスツリーレベルを結合して形成する。トップレベルの `absolutePath=` エントリは既に完全であり、そのまま使用する。
{% if has_predecessor_attachments %}
### Predecessor chain（現在ノードを作成したタスク引き渡しチェーン。最大 {{ source_predecessor_limit }} ノード）
{{ predecessor_attachments }}
{% if has_predecessor_attachment_overflow %}
- 以下ソースノードの添付列挙は切り詰めまたは不完全です。ノードあたり最大 {{ attachments_per_source_limit }} ファイルまたは空ディレクトリを検査。空でないディレクトリは走査され、スロット自体は消費しない。上記には見つかったファイルのみ列挙。必要に応じて完全な attachments ディレクトリを検査してください：
{{ predecessor_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_dependency_attachments %}
### Explicit dependencies（現在ノードが dependsOn で明示的に命名した入力ノード）
{{ dependency_attachments }}
{% if has_dependency_attachment_overflow %}
- 以下ソースノードの添付列挙は切り詰めまたは不完全です。ノードあたり最大 {{ attachments_per_source_limit }} ファイルまたは空ディレクトリを検査。空でないディレクトリは走査され、スロット自体は消費しない。上記には見つかったファイルのみ列挙。必要に応じて完全な attachments ディレクトリを検査してください：
{{ dependency_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_group_evidence_attachments %}
### Group evidence（現在 merge / acceptance 入力、または関連グループの最新 merge と acceptance）
{{ group_evidence_attachments }}
{% if has_group_evidence_attachment_overflow %}
- 以下ソースノードの添付列挙は切り詰めまたは不完全です。ノードあたり最大 {{ attachments_per_source_limit }} ファイルまたは空ディレクトリを検査。空でないディレクトリは走査され、スロット自体は消費しない。上記には見つかったファイルのみ列挙。必要に応じて完全な attachments ディレクトリを検査してください：
{{ group_evidence_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% endif %}

{% if has_output_contract %}
## Session reuse
- Session mode: {{ session_mode }}
- continueFromNodeId: {{ continue_from_node_id }}
- 注：`continue` はソースノードの ACP セッションコンテキストのみ再利用する。現在のタスクは本 user プロンプトの `# Task` です。
- 現在チェーン内で再開可能なセッションノード：
{{ resumable_sessions }}

## Runtime limits
- Allowed workflow snapshots:
{{ allowed_workflow_snapshots }}
- Remaining budget:
{{ remaining_budget }}

## Agent and profile options
- Dynamic node agent strategy: {{ agent_strategy_mode }}
- Bootstrap agent: {{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Agent routing guidance:
{{ agent_routing_prompt }}
- Merge / acceptance model policy:
{{ acceptance_model_policy }}
{% endif %}- Available agents and configured runtime options:
{{ available_providers }}
- Available profiles:
{{ available_profiles }}
{% endif %}
