前回の `dynamic-node-completion` 提案は受理されていません。以下の検証項目またはリマインダーに対処し、再提出してください。

以下の runtime 制約を満たすよう、最終 `dynamic-node-completion` 出力を修復する必要があります。
プロトコル検証エラーのみ修復する。タスクを再実行しない。後続作業は引き続きスコープ契約を満たす必要がある。スコープ外項目を削除または縮小する。
{% if fanout_workspace_dirty %}
この fanout は HEAD から worktree を作成しようとしています。ソースワークスペースに未コミットコードが検出されたため、以下に注意してください：
- Fork 元ワークスペース: {{ fanout_workspace_path }}。本タスクに後続ブランチに必要な未コミット業務変更があるか確認する。ある場合は、Conventional Commits を任意で使用し、それらの特定パスをレビューしてコミットする。
- コミット不要な場合は Git 操作を行わず、artifact を直接再提出する。クリーンなワークスペースも新規コミットも不要。残りの dirty ファイルは fanout を再度 BLOCKER にしない。
- このリマインダーのためにワークスペースをクリーンアップしたり、無関係な内容を stash したり、他の worktree を移動したり、`git add -A` を盲目的に使用したり、ignore ルールを変更したりしない。無関係な内容と安全に扱えない変更を保持する。
- 再提出 artifact は他のすべてのプロトコル検証も引き続き合格する必要がある。スキーマを変更したり、workspace/branch フィールドを追加したりしない。
{% endif %}
説明、Markdown、コードフェンス、または余分なテキストを出力しない。修復された `dynamic-node-completion` 内容のみを出力する。

{% if has_coordination_snapshot %}最新 coordination スナップショット：
- 読み取り専用スナップショット: {{ coordination_snapshot_path }}
- 修復して `next.type="single"` または `next.type="fanout"` を出力する前に最新 coordination スナップショットを読み取る。読み取りのみで、このファイルを変更しない。
{% endif %}

検証エラー：
{{ validation_errors }}

現在の有効値参照：
{{ repair_reference }}

現在の残予算：
{{ remaining_budget }}
