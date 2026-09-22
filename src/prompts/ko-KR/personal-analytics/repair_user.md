이번 작업의 개인 데이터 분석 내러티브 객체를 수정하십시오.

operationId: {{ operation_id }}
유효하지 않은 보고서 파일: {{ invalid_report_path }}

검증 오류:

{{ validation_errors }}

대상 JSON Schema:

{{ report_schema }}

유효하지 않은 보고서 파일을 읽고, 위 schema를 만족하는 데 필요한 구조 수정만 수행한 뒤, 수정된 JSON 객체만 출력하십시오.
