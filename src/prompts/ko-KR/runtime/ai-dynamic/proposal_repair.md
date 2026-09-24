이전 `dynamic-node-completion` proposal이 아직 수락되지 않았습니다. 아래 검증 항목 또는 알림을 처리한 뒤 다시 제출하십시오.

최종 `dynamic-node-completion` 출력을 아래 runtime 제약을 만족하도록 수정해야 합니다.
프로토콜 검증 오류만 수정하고 작업을 다시 실행하지 마십시오. 후속 작업은 여전히 범위 계약을 만족해야 하며, 범위 밖 항목은 제거하거나 축소하십시오.
{% if fanout_workspace_dirty %}
이번 fanout은 HEAD에서 worktree를 만들 예정입니다. 소스 workspace에 미커밋 코드가 감지되어 다음을 참고하십시오:
- Fork 소스 workspace: {{ fanout_workspace_path }}. 이번 작업에서 생성되었고 후속 branch에 필요한 미커밋 비즈니스 변경이 있는지 확인하십시오. 있다면 해당 경로만 검토 후 커밋할 수 있으며 Conventional Commits를 사용할 수 있습니다.
- 커밋할 내용이 없으면 Git 작업을 하지 말고 artifact를 다시 제출하십시오. workspace가 깨끗할 필요도 새 commit이 필요하지도 않으며, 남은 dirty 파일로 fanout이 다시 막히지 않습니다.
- workspace 정리, 무관 stash, 다른 worktree 이동, 무분별한 `git add -A`, ignore 규칙 변경을 이 알림 때문에 하지 마십시오. 무관 콘텐츠와 안전하게 처리할 수 없는 변경은 그대로 두십시오.
- 다시 제출하는 artifact는 다른 모든 프로토콜 검증도 통과해야 하며, schema를 바꾸거나 workspace/branch 필드를 추가하지 마십시오.
{% endif %}
설명, Markdown, 코드 펜스, 추가 텍스트를 출력하지 말고, 수정된 `dynamic-node-completion` 내용만 출력하십시오.

{% if has_coordination_snapshot %}최신 coordination snapshot:
- 읽기 전용 snapshot: {{ coordination_snapshot_path }}
- 수정·출력 전 `next.type="single"` 또는 `next.type="fanout"`을 위해 최신 coordination snapshot을 읽으십시오. 읽기만 하고 파일을 수정하지 마십시오.
{% endif %}

검증 오류:
{{ validation_errors }}

현재 유효 값 참조:
{{ repair_reference }}

현재 남은 예산:
{{ remaining_budget }}
