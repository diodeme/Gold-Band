生成本次个人数据分析洞察扩展。

operationId：{{ operation_id }}
reportSchemaVersion：{{ report_schema_version }}
sourceWatermark：{{ source_watermark }}
indexRevision：{{ index_revision }}
日期范围：{{ date_range }}

授权输入：

- 全历史统计投影：{{ projection_path }}
- 内容授权清单：{{ content_manifest_path }}
- 语义批次清单：{{ semantic_batch_manifest_path }}

客户端预检覆盖摘要：

{{ coverage_summary }}

先读取当前日期范围的统计投影，再处理语义批次附件中已经内联的有界内容。内容清单中的 locator 只用于证据引用，不允许据此读取原始文件。不要扫描上述路径的父目录，也不要自行扩大时间范围、文件范围或批次预算。

以 `reportSchemaVersion` 输出洞察对象。不要复述确定性统计；只返回有证据的分章节洞察。
