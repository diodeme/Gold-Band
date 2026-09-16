修复本次个人数据分析洞察对象。

operationId：{{ operation_id }}
无效报告文件：{{ invalid_report_path }}

校验错误：

{{ validation_errors }}

目标 JSON Schema：

{{ report_schema }}

读取无效报告文件，仅进行满足上述 schema 所必需的结构修复，并只输出修复后的 JSON 对象。
