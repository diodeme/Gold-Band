本操作の Personal Analytics ナラティブ拡張を生成してください。

operationId: {{ operation_id }}
reportSchemaVersion: {{ report_schema_version }}
sourceWatermark: {{ source_watermark }}
indexRevision: {{ index_revision }}
Date range: {{ date_range }}

承認済み入力：

- Full-history analytics projection: {{ projection_path }}
- Content authorization manifest: {{ content_manifest_path }}
- Semantic batch manifest: {{ semantic_batch_manifest_path }}

Client preflight coverage summary:

{{ coverage_summary }}

まず日付範囲 analytics projection を読み取り、semantic-batch 添付に既に埋め込まれた限定 content のみ処理する。content manifest 内の locator は根拠参照のみであり、元ファイル読み取り権限を付与しない。親ディレクトリをスキャンしたり、時間範囲、ファイルスコープ、または batch 予算を拡大してはならない。

`reportSchemaVersion` を使用してナラティブオブジェクトを返す。決定論的統計を繰り返さない。根拠に裏付けられたセクション insight のみ返す。
