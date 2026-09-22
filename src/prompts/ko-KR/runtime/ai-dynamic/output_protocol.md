마지막 단계에서는 `dynamic-node-completion` artifact에 해당하는 JSON 내용만 출력하십시오. 설명, Markdown, 코드 펜스, 추가 텍스트를 출력하지 마십시오.

{% if agent_strategy_mode == "fixed" %}
현재 AI-DYNAMIC은 고정 Agent 전략을 사용합니다. `workflow-invocation`을 제외한 모든 internal worker, merge, acceptance 노드는 runtime이 선택한 동일한 고정 provider를 사용합니다. 어떤 노드에도 provider 필드를 출력하지 마십시오.
{{ model_policy }}
{% else %}
현재 AI-DYNAMIC은 동적 Agent 전략을 사용합니다. 이 prompt의 라우팅 가이드와 사용 가능 provider에 따라 후속 worker에만 provider를 선택·출력하십시오. merge / acceptance는 항상 bootstrap Agent를 사용하므로 provider를 출력하지 마십시오. 어떤 노드에도 `model` 또는 `permissionMode`를 출력하지 마십시오. runtime이 저장된 구성을 읽습니다.
{{ model_policy }}
{% endif %}

아래 JSON Schema는 이번 run의 유효 출력 프로토콜입니다. runtime이 Rust 데이터 구조에서 생성하고 현재 AI-DYNAMIC 구성으로 좁혔습니다. 출력은 이를 만족해야 하며 runtime도 동일 schema로 검증·repair 진단합니다.

```json
{{ json_schema }}
```

제약 알림:
- 후속 작업은 확립된 범위 내 결과를 분해하거나 qualified `BLOCKER`만 수정할 수 있습니다. `FOLLOW_UP`이나 전행 제안을 새 outcome으로 승격하지 마십시오. scope drift는 최소 범위 내 솔루션 복원만 배치할 수 있습니다.
{% if agent_strategy_mode == "fixed" %}- 고정 Agent 전략에서는 어떤 `provider` 필드도 출력하지 마십시오. runtime이 고정 Agent를 자동 주입합니다.
{% else %}- 동적 Agent 전략에서는 worker가 이 prompt의 라우팅 가이드를 따르는 유효 provider를 출력해야 합니다. `merge / acceptance`는 provider를 생략하십시오. runtime이 항상 bootstrap Agent를 사용합니다.
- `workflow-invocation`에는 `provider`를 출력하지 마십시오.
{% endif %}- {{ model_policy }}
- `next.type="end"`일 때 `next`에 `node / groupId / nodes / merge / acceptance`를 넣지 마십시오.
{% if end_summary_is_outer_handoff %}- `next.type="end"`를 사용하면 `summary`는 AI-DYNAMIC 외부 후속 노드에 대한 완전한 비즈니스 인계 요약이어야 합니다. 완료 내용, 핵심 결론, 중요 산출물, 남은 우려를 기술하고, 라우팅 동작이나 "acceptance 통과"만 쓰지 마십시오.
{% else %}- `next.type="end"`를 사용하면 `summary`는 내부 진행/branch 보고입니다. Runtime 보고 manifest와 상위 group을 위해 이 노드가 완료한 내용을 정확히 기술하십시오.
{% endif %}
- `next.type="single"`이면 완전한 `next.node`를 제공하고 `groupId / nodes / merge / acceptance`는 제공하지 마십시오.
- 어떤 노드에도 `workspace`, workspace mode, 경로, branch를 출력하지 마십시오. runtime이 workspace 할당을 독점합니다.
- `next.type="single"` 후속은 현재 노드의 실제 workspace를 자동 상속합니다.
- 현재 노드가 group acceptance이면 유효 출력이 수락될 때 해당 group이 닫힙니다. `single`은 부모 scope의 원래 비즈니스 branch로 복귀하고, `fanout`은 부모 scope에 새 group을 만들며, `end`만 해당 branch를 종료합니다. 후속이 있으면 부모 group은 계속 대기하며, repair와 재검증을 명시적으로 배치해야 하고 이전 group은 자동으로 다시 열리지 않습니다.
- `next.type="fanout"`이면 `groupId / nodes / merge / acceptance`를 함께 제공하고 `nodes`는 최소 두 branch를 포함해야 합니다. 후속 노드가 하나면 `next.type="single"`을 사용하십시오.
- `next.type="fanout"`의 각 child는 격리 worktree를 자동 받습니다. merge와 acceptance는 해당 group의 부모 workspace로 자동 복귀합니다.
- fanout child worktree는 동일한 커밋된 revision만 상속하며 미커밋 내용은 상속하지 않습니다. Runtime은 자동 checkpoint하지 않습니다. 후속 branch에 필요한 미커밋 비즈니스 변경이 있으면 해당 경로만 검토 후 커밋할 수 있으며 Conventional Commits를 사용할 수 있습니다. 커밋할 것이 없으면 Git 작업을 하지 마십시오.
- workspace가 깨끗한 것은 fanout 게이트가 아닙니다. Runtime은 dirty workspace를 처음 감지할 때 한 번만 알리며, 이후 artifact를 다시 제출하면 됩니다. 새 commit은 필요하지 않습니다. workspace 정리, 무관 stash, 다른 worktree 이동, 무분별한 `git add -A`, ignore 규칙 변경을 하지 마십시오. 이번 delivery와 무관한 내용은 그대로 두십시오.
- `profile`은 worker 노드에서만 허용되며 선택입니다. 있으면 schema enum 또는 이 prompt의 `profileId=...` 뒤 ID를 사용하고 displayName을 쓰지 마십시오.
- `merge` / `acceptance`에는 `profile`을 출력하지 마십시오. runtime 내장 AI-DYNAMIC merge / acceptance prompt를 사용합니다.
{% if agent_strategy_mode == "dynamic" %}- `provider`가 있으면 schema enum 또는 이 prompt에 나열된 사용 가능 provider 중 하나여야 합니다.
{% endif %}- `sessionMode`를 생략하면 `new`로 처리합니다. 현재 체인의 재사용 가능 세션 노드를 이어갈 때만 `continue`를 사용하십시오.
- `sessionMode="continue"`이면 `continueFromNodeId`를 제공해야 하며, 이 prompt에 나열된 재사용 가능 세션 노드만 참조할 수 있습니다.
- `workflow-invocation`에는 `sessionMode="continue"`를 사용하지 마십시오.
- `workflowId`가 있으면 schema enum 또는 이 prompt의 allowed workflow DSL ID 중 하나여야 합니다.
- fanout 노드 수는 schema `minItems/maxItems`, 이 prompt의 `maxFanout`, 남은 예산 제약을 만족해야 합니다.
- 의사 코드, 설명, 예시 래퍼 없이 최종 JSON만 출력하십시오.
