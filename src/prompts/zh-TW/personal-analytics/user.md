產生本次個人資料分析洞察擴展。

operationId：{{ operation_id }}
reportSchemaVersion：{{ report_schema_version }}
sourceWatermark：{{ source_watermark }}
indexRevision：{{ index_revision }}
日期範圍：{{ date_range }}

授權輸入：

- 全歷史統計投影：{{ projection_path }}
- 內容授權清單：{{ content_manifest_path }}
- 語意批次清單：{{ semantic_batch_manifest_path }}

用戶端預檢覆蓋摘要：

{{ coverage_summary }}

先讀取目前日期範圍的統計投影，再處理語意批次附件中已經內嵌的有界內容。內容清單中的 locator 只用於證據引用，不允許據此讀取原始檔案。不要掃描上述路徑的父目錄，也不要自行擴大時間範圍、檔案範圍或批次預算。

以 `reportSchemaVersion` 輸出洞察物件。不要複述確定性統計；只回傳有證據的分章節洞察。
