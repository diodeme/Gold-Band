Gere a extensão narrativa de personal analytics para esta operação.

operationId: {{ operation_id }}
reportSchemaVersion: {{ report_schema_version }}
sourceWatermark: {{ source_watermark }}
indexRevision: {{ index_revision }}
Date range: {{ date_range }}

Entradas autorizadas:

- Projeção de analytics de histórico completo: {{ projection_path }}
- Manifesto de autorização de conteúdo: {{ content_manifest_path }}
- Manifesto de lote semântico: {{ semantic_batch_manifest_path }}

Resumo de cobertura de preflight do cliente:

{{ coverage_summary }}

Leia primeiro a projeção de analytics do intervalo de datas, depois processe apenas o conteúdo limitado já embutido no attachment de lote semântico. Localizadores no manifesto de conteúdo são apenas referências de evidência e não concedem permissão para ler arquivos originais. Não escaneie nenhum diretório pai nem expanda intervalo de tempo, escopo de arquivo ou orçamento de lote.

Retorne o objeto narrativo usando `reportSchemaVersion`. Não repita estatísticas determinísticas; retorne apenas insights seccionados respaldados por evidência.
