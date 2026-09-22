이번 응답 turn이 종료되었지만 runtime이 아직 본 노드의 artifact를 받지 못했습니다. 아래는 출력 프로토콜을 제공하거나 재확인하는 것이며, 작업이 이미 완료되었거나 조기 인계를 요구하는 것은 아닙니다. 현재 실행 의도에 따라 본 노드 종료 여부를 결정하십시오.

- 본 노드를 종료할 의도가 없다면 현재 작업 범위·workspace·도구 권한을 유지한 채 바로 계속 실행하십시오. 상태 태그는 필요 없으며, 이 prompt에 응답하기 위해 조기 인계하지 마십시오. 계속 실행한 뒤 본 노드가 끝났다고 판단할 때 아래 artifact를 출력하십시오.
- 본 노드가 끝났다고 판단하면 완료된 작업을 바탕으로 아래 artifact를 출력하십시오. 작업 목표나 acceptance 요건을 다시 감사하거나, 이 prompt 때문에 비즈니스 작업을 추가하지 마십시오.
- artifact 출력 전, 현재 작업에 보고서 등 첨부가 필요하고 아직 쓰지 않았다면 이번 attempt의 attachments 디렉터리에 작성하십시오. 불필요하거나 이미 완료되었으면 생략하십시오.
{% if can_read_runtime_snapshot %}- artifact 준비 시, 아래 runtime context가 명시적으로 요구할 때만 읽기 전용 runtime snapshot을 갱신하고, 선언된 snapshot 경로만 읽으십시오.
{% endif %}- 계속 실행 중에는 정상적으로 응답하고 도구를 사용할 수 있습니다. 최종 artifact 출력 시에만 설명, Markdown, 코드 펜스를 붙이지 마십시오.
{% if finalize_context %}
아래 runtime context는 이번 제어 결과 정규화에만 사용됩니다:
{{ finalize_context }}
{% endif %}

출력 artifact: {{ artifact }}
출력 종류: {{ kind }}

본 노드가 끝났고 artifact 출력을 결정했을 때만 아래 프로토콜을 따르십시오:
{{ schema }}{% if success_condition %}

runtime은 이후 아래 조건으로 노드 결과를 판단합니다:
{{ success_condition }}{% endif %}
