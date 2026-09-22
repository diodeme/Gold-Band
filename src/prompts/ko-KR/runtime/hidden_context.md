# 이번 Gold Band runtime context

- 세션 모드: {{ session_mode }}
- Round: {{ round_id }}
- Attempt: {{ attempt_id }}
- Attempt 디렉터리: {{ attempt_dir }}
- Attachments 디렉터리(본 노드 보고서·임시 스크립트·과정 기록 등 자유 출력의 기본 위치): {{ attachments_dir }}
{% if invocation_reason %}
- 호출 사유: {{ invocation_reason }}
{% endif %}

{% if predecessors.is_empty %}
## 최신 전행 실행 체인
이전 실행 노드: 없음. 현재 노드는 이번 round의 진입 노드입니다.
{% else %}
## 최신 전행 실행 체인
{{ predecessors.chain }}
{% endif %}

{% if predecessors.reason_lines_empty %}
{% if predecessors.is_empty %}
## 최신 전행 전환 사유
없음.
{% else %}
## 최신 전행 전환 사유
이전 노드는 모두 일반 전환이며 노드 결과에 따라 현재 분기로 진입했습니다.
{% endif %}
{% else %}
## 최신 전행 전환 사유
{{ predecessors.reason_lines }}
{% endif %}

{% if predecessors.has_ai_dynamic_report_manifest %}
## AI-DYNAMIC 전체 보고서 manifest(필요 시 읽기)
전행 `ai-dynamic-result`의 `reportManifest.path`는 노드/group 토폴로지, 의존·시간 관계, workspace, 내부 summary, 첨부 locator를 포함하는 전체 내부 실행 보고서 색인을 가리킵니다. 기본적으로 비즈니스 인계 `summary`를 사용하고, 내부 과정 확인·보고서 첨부 탐색·`summary` 정보 부족 시에만 manifest를 읽으십시오.
{% endif %}

{% if not predecessors.attachment_lines_empty %}
## 최신 전행 첨부
{{ predecessors.attachment_lines }}
{% endif %}
