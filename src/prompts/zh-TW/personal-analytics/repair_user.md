修復本次個人資料分析洞察物件。

operationId：{{ operation_id }}
無效報告檔案：{{ invalid_report_path }}

校驗錯誤：

{{ validation_errors }}

目標 JSON Schema：

{{ report_schema }}

讀取無效報告檔案，僅進行滿足上述 schema 所必需的結構修復，並只輸出修復後的 JSON 物件。
