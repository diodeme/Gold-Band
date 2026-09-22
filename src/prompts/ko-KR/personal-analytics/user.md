이번 작업의 개인 데이터 분석 내러티브 확장을 생성하십시오.

operationId: {{ operation_id }}
reportSchemaVersion: {{ report_schema_version }}
sourceWatermark: {{ source_watermark }}
indexRevision: {{ index_revision }}
날짜 범위: {{ date_range }}

승인된 입력:

- 전체 이력 분석 projection: {{ projection_path }}
- 콘텐츠 승인 manifest: {{ content_manifest_path }}
- 시맨틱 배치 manifest: {{ semantic_batch_manifest_path }}

클라이언트 사전 검사 커버리지 요약:

{{ coverage_summary }}

먼저 현재 날짜 범위의 분석 projection을 읽고, 시맨틱 배치 첨부에 이미 인라인된 유계 콘텐츠만 처리하십시오. 콘텐츠 manifest의 locator는 증거 참조용이며 원본 파일 읽기 권한을 부여하지 않습니다. 위 경로의 상위 디렉터리를 스캔하거나, 시간 범위·파일 범위·배치 예산을 임의로 확대하지 마십시오.

`reportSchemaVersion`에 맞는 내러티브 객체를 반환하십시오. 결정론적 통계를 반복하지 말고, 증거가 뒷받침하는 섹션별 인사이트만 반환하십시오.
