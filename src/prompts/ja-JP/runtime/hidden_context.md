# 本呼び出しの Gold Band runtime コンテキスト

- Session mode: {{ session_mode }}
- Round: {{ round_id }}
- Attempt: {{ attempt_id }}
- Attempt directory: {{ attempt_dir }}
- Attachments directory（本ノードのレポート、一時スクリプト、プロセスメモ、その他自由形式出力の既定場所）: {{ attachments_dir }}
{% if invocation_reason %}
- Invocation reason: {{ invocation_reason }}
{% endif %}

{% if predecessors.is_empty %}
## 最新先行チェーン
以前実行されたノード：なし。本ノードは現在ラウンドのエントリノードです。
{% else %}
## 最新先行チェーン
{{ predecessors.chain }}
{% endif %}

{% if predecessors.reason_lines_empty %}
{% if predecessors.is_empty %}
## 最新先行遷移理由
なし。
{% else %}
## 最新先行遷移理由
以前のすべてのノードは、ノード結果に基づく通常遷移でした。
{% endif %}
{% else %}
## 最新先行遷移理由
{{ predecessors.reason_lines }}
{% endif %}

{% if predecessors.has_ai_dynamic_report_manifest %}
## AI-DYNAMIC 完全レポート manifest（必要時読み取り）
先行 `ai-dynamic-result` 内の `reportManifest.path` は、ノード/グループトポロジ、依存関係とタイミング、ワークスペース、内部要約、添付ロケータを含む完全な内部実行レポート索引を指す。既定では業務引き渡し `summary` を使用する。内部実行の検証、レポート添付の特定、または `summary` に必要な詳細が不足している場合のみ manifest を読み取る。
{% endif %}

{% if not predecessors.attachment_lines_empty %}
## 最新先行添付
{{ predecessors.attachment_lines }}
{% endif %}
