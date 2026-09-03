Generate the personal analytics narrative extension for this operation.

operationId: {{ operation_id }}
reportSchemaVersion: {{ report_schema_version }}
sourceWatermark: {{ source_watermark }}
indexRevision: {{ index_revision }}
Date range: {{ date_range }}

Authorized inputs:

- Full-history analytics projection: {{ projection_path }}
- Content authorization manifest: {{ content_manifest_path }}
- Semantic batch manifest: {{ semantic_batch_manifest_path }}

Client preflight coverage summary:

{{ coverage_summary }}

Read the date-range analytics projection first, then process only the bounded content already embedded in the semantic-batch attachment. Locators in the content manifest are evidence references only and do not grant permission to read original files. Do not scan any parent directory or expand the time range, file scope, or batch budget.

Return the narrative object using `reportSchemaVersion`. Do not repeat deterministic statistics; return only sectioned insights supported by evidence.
