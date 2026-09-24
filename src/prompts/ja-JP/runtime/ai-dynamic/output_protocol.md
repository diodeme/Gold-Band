最終ステップでは `dynamic-node-completion` artifact の JSON 内容のみを出力する。説明、Markdown、コードフェンス、または余分なテキストを出力してはならない。

{% if agent_strategy_mode == "fixed" %}
本 AI-DYNAMIC ノードは fixed-agent 戦略を使用する：`workflow-invocation` を除くすべての内部 worker、merge、acceptance ノードは、runtime が選択した同一 fixed provider を使用する。いかなるノードにも provider フィールドを出力しない。
{{ model_policy }}
{% else %}
本 AI-DYNAMIC ノードは dynamic-agent 戦略を使用する：本プロンプトのルーティングガイダンスと利用可能 provider に基づき、後続 worker のみ provider を選択して出力する。Merge / acceptance は常に bootstrap Agent を使用するため、provider を出力しない。いかなるノードにも `model` または `permissionMode` を出力しない。runtime は保存済み設定を読み取る。
{{ model_policy }}
{% endif %}

以下の JSON Schema は本 run の有効出力プロトコルです。Runtime は Rust データ構造から生成し、現在の AI-DYNAMIC 設定で絞り込みました。出力はこれを満たす必要があり、runtime は検証と修復診断に同じスキーマを使用する。

```json
{{ json_schema }}
```

制約リマインダー：
- 後続タスクは、確立されたスコープ内成果の分解、または適格 `BLOCKER` の修復のみ可能。`FOLLOW_UP` または先行提案を新たな成果に昇格させない。スコープ逸脱は最小限のスコープ内解決の復元のみスケジュール可能。
{% if agent_strategy_mode == "fixed" %}- fixed-agent 戦略では、いかなる `provider` フィールドも出力しない。Runtime が fixed agent を自動注入する。
{% else %}- dynamic-agent 戦略では、worker は本プロンプトのルーティングガイダンスに従う有効 provider を出力する必要がある。`merge / acceptance` は runtime が常に bootstrap Agent を使用するため provider を省略する。
- `workflow-invocation` には `provider` を出力しない。
{% endif %}- {{ model_policy }}
- `next.type="end"` の場合、`node / groupId / nodes / merge / acceptance` を含めない。
{% if end_summary_is_outer_handoff %}- `next.type="end"` を使用する場合、`summary` は AI-DYNAMIC 外の後続への完全な業務引き渡しであること：完了内容、主要結論、重要出力、残る懸念を述べる。ルーティングの説明のみ、または「accepted」と言うだけにしてはならない。
{% else %}- `next.type="end"` を使用する場合、`summary` は内部進捗またはブランチレポートである。Runtime レポート manifest と包含グループ向けに本ノードが完了した内容を正確に述べる。
{% endif %}
- `next.type="single"` の場合、完全な `next.node` を提供し、`groupId / nodes / merge / acceptance` を提供してはならない。
- いかなるノードにも `workspace`、ワークスペースモード、パス、またはブランチを出力しない。Runtime がワークスペース割り当てを独占所有する。
- `next.type="single"` 後続は現在ノードの実際のワークスペースを自動継承する。
- 本ノードがグループ acceptance の場合、有効出力の受理でそのグループは閉じる：`single` は親スコープの元業務ブランチを再開、`fanout` は親スコープに新グループを作成、`end` のみそのブランチを終了。後続がある場合、親グループは待機を続ける。修復と検証を明示的に手配する。旧グループは自動的に再開されない。
- `next.type="fanout"` の場合、`groupId / nodes / merge / acceptance` をまとめて提供し、`nodes` には少なくとも 2 ブランチを含める。後続ノードが 1 つの場合は `next.type="single"` を使用する。
- すべての `next.type="fanout"` 子は隔離 worktree を自動受信する。merge と acceptance はそのグループの親ワークスペースに自動復帰する。
- Fanout 子 worktree は 1 つのコミット済み revision を継承し、未コミット内容は継承しない。Runtime は自動チェックポイントしない。後続ブランチに必要な未コミット業務変更がある場合、Conventional Commits を任意で使用し、それらの特定パスをレビューしてコミットする。コミット不要な場合は Git 操作を行わない。
- クリーンなワークスペースは fanout ゲートではない。Runtime は dirty ワークスペース初回検出時に 1 回リマインダーを出す。その後 artifact を再提出する。新規コミットは不要。ワークスペースをクリーンアップしたり、無関係な内容を stash したり、他 worktree を移動したり、`git add -A` を盲目的に使用したり、このリマインダーのために ignore ルールを変更したりしない。無関係な内容はそのまま残す。
- `profile` は worker ノードでのみ許可され、任意。存在する場合、スキーマ enum の ID または本プロンプトの `profileId=...` 後の ID を使用し、displayName ではない。
- `merge` / `acceptance` には `profile` を出力しない。runtime は組み込み AI-DYNAMIC merge / acceptance プロンプトを使用する。
{% if agent_strategy_mode == "dynamic" %}- `provider` が存在する場合、スキーマ enum または本プロンプトに列挙された利用可能 provider の 1 つである必要がある。
{% endif %}- `sessionMode` を省略した場合、`new` として扱われる。現在チェーン内の再利用可能セッションノードを再開する場合のみ `continue` を使用する。
- `sessionMode="continue"` の場合、`continueFromNodeId` を提供し、本プロンプトに列挙された再開可能セッションノードの 1 つを参照する必要がある。
- `workflow-invocation` には `sessionMode="continue"` を使用しない。
- `workflowId` が存在する場合、スキーマ enum または本プロンプトに列挙された許可 Workflow DSL ID の 1 つである必要がある。
- Fanout ノード数は、本プロンプトに示されたスキーマ `minItems/maxItems`、`maxFanout`、残予算制約を満たす必要がある。
- 最終 JSON のみ出力する。疑似コード、解説、ラップされた例を出力しない。
