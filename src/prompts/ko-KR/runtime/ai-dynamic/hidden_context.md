# 이번 AI-DYNAMIC runtime context

## 현재 동적 노드
- 부모 노드: {{ outer_node_id }}
- 부모 attempt: {{ outer_attempt_id }}
- Dynamic run: {{ dynamic_run_id }}
- 내부 노드: {{ node_id }}
- 제목: {{ title }}
- 종류: {{ kind }}
- Group: {{ group_id }}
- Chain: {{ chain_id }}
- 깊이: {{ depth }}

## 실행 위치
- Dynamic 루트: {{ dynamic_root }}
- 내부 노드(Dynamic 루트 기준): {{ node_dir }}
- 현재 attempt(내부 노드 기준): {{ attempt_dir }}
- Attachments(현재 attempt 기준): {{ attachments_dir }}
- Workspace ID: {{ workspace_id }}
- Workspace 경로: {{ workspace_path }}
- Workspace capability:
{{ workspace_capability }}

{% if has_new_round_trigger %}
## `$new-round` 트리거 피드백
{{ new_round_trigger }}
- 현재 새 Round를 연 실패 노드 출력입니다. 실패 사유와 미완료 항목을 먼저 이해한 뒤 이번 round 내부 작업을 계획하고, 원래 요구사항을 그대로 재실행하지 마십시오.
- artifact 미리보기는 잘릴 수 있습니다. 전체 정보가 필요하면 위에 명시된 artifact나 첨부를 읽으십시오.
{% endif %}

{% if has_coordination_snapshot %}
## Runtime coordination snapshot
- 읽기 전용 snapshot(Dynamic 루트 기준): {{ coordination_snapshot_path }}
- Runtime이 canonical dynamic graph에서 이 파일을 생성하며 유일한 writer입니다. 수정하지 마십시오.
- 작업 시작·계속 전 최신 snapshot을 읽으십시오. 각 `workstreams[]`의 goal, TODO 상태, 부모 관계, steps로 다른 하위 작업을 이해하고, `groups[]` 중첩과 phase로 중복·충돌 작업을 피하십시오.
- `next.type="single"` 또는 `next.type="fanout"` 출력 전 같은 경로를 다시 읽고 최신 상태로 후속을 계획하십시오.
{% endif %}

{% if has_direct_predecessors %}
## 직접 전행 노드
{{ direct_predecessors }}
{% endif %}

{% if has_active_group %}
## 활성 group
{{ active_group }}
{% endif %}

{% if has_inherited_groups %}
## 상속된 group context
{{ inherited_groups }}
{% endif %}

{% if has_siblings %}
## 병렬 sibling
{{ siblings }}
{% endif %}

{% if has_available_attachments %}
## 사용 가능한 첨부
- 아래는 첨부 경로만 나열하며 본문은 읽거나 인라인하지 않습니다. 일반 항목의 전체 경로는 `Dynamic 루트`와 경로 트리 각 레벨을 순서대로 이어 붙입니다. 최상위 `absolutePath=` 항목은 이미 전체 경로이므로 그대로 사용하십시오.
{% if has_predecessor_attachments %}
### 전행 체인(현재 노드를 만든 작업 인계 체인, 최대 {{ source_predecessor_limit }}개 노드)
{{ predecessor_attachments }}
{% if has_predecessor_attachment_overflow %}
- 아래 소스 노드의 첨부 목록은 잘렸거나 불완전합니다. 노드당 최대 {{ attachments_per_source_limit }}개 파일 또는 빈 디렉터리를 검사하며, 내용 있는 디렉터리는 재귀 탐색하고 슬롯을 소비하지 않습니다. 위에는 찾은 파일만 나열되었으므로 필요 시 전체 attachments 디렉터리를 확인하십시오:
{{ predecessor_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_dependency_attachments %}
### 명시적 의존(현재 노드가 dependsOn으로 지정한 입력 노드)
{{ dependency_attachments }}
{% if has_dependency_attachment_overflow %}
- 아래 소스 노드의 첨부 목록은 잘렸거나 불완전합니다. 노드당 최대 {{ attachments_per_source_limit }}개 파일 또는 빈 디렉터리를 검사하며, 내용 있는 디렉터리는 재귀 탐색하고 슬롯을 소비하지 않습니다. 위에는 찾은 파일만 나열되었으므로 필요 시 전체 attachments 디렉터리를 확인하십시오:
{{ dependency_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_group_evidence_attachments %}
### Group 증거(현재 merge / acceptance 입력 또는 관련 group의 최근 merge·acceptance)
{{ group_evidence_attachments }}
{% if has_group_evidence_attachment_overflow %}
- 아래 소스 노드의 첨부 목록은 잘렸거나 불완전합니다. 노드당 최대 {{ attachments_per_source_limit }}개 파일 또는 빈 디렉터리를 검사하며, 내용 있는 디렉터리는 재귀 탐색하고 슬롯을 소비하지 않습니다. 위에는 찾은 파일만 나열되었으므로 필요 시 전체 attachments 디렉터리를 확인하십시오:
{{ group_evidence_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% endif %}

{% if has_output_contract %}
## 세션 재사용
- Session mode: {{ session_mode }}
- continueFromNodeId: {{ continue_from_node_id }}
- 참고: `continue`는 소스 노드의 ACP session context만 재사용합니다. 현재 작업은 이번 user prompt의 `# 작업`을 따릅니다.
- 현재 체인에서 재사용 가능한 세션 노드:
{{ resumable_sessions }}

## Runtime 제한
- Allowed workflow snapshots:
{{ allowed_workflow_snapshots }}
- 남은 예산:
{{ remaining_budget }}

## Agent 및 Profile 옵션
- 동적 노드 Agent 전략: {{ agent_strategy_mode }}
- Bootstrap Agent: {{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Agent 라우팅 가이드:
{{ agent_routing_prompt }}
- merge / acceptance 모델 정책:
{{ acceptance_model_policy }}
{% endif %}- 사용 가능한 Agent 및 구성된 runtime 옵션:
{{ available_providers }}
- 사용 가능한 Profile:
{{ available_profiles }}
{% endif %}
