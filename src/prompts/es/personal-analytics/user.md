Genera la extensión narrativa de análisis personal para esta operación.

operationId: {{ operation_id }}
reportSchemaVersion: {{ report_schema_version }}
sourceWatermark: {{ source_watermark }}
indexRevision: {{ index_revision }}
Rango de fechas: {{ date_range }}

Entradas autorizadas:

- Proyección analítica de historial completo: {{ projection_path }}
- Manifiesto de autorización de contenido: {{ content_manifest_path }}
- Manifiesto de lote semántico: {{ semantic_batch_manifest_path }}

Resumen de cobertura de preflight del cliente:

{{ coverage_summary }}

Lee primero la proyección analítica del rango de fechas; después procesa solo el contenido acotado ya incrustado en el attachment de lote semántico. Los localizadores del manifiesto de contenido son solo referencias de evidencia y no conceden permiso para leer archivos originales. No escanees ningún directorio padre ni amplíes el rango temporal, el alcance de archivos o el presupuesto de lote.

Devuelve el objeto narrativo usando `reportSchemaVersion`. No repitas estadísticas deterministas; devuelve solo insights por secciones respaldados por evidencia.
