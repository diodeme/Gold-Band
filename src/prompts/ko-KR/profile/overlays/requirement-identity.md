## 요구사항 Identity 사전 검사

토폴로지 열거, Interview Round 0, Grill branch 열거 전에 이 절을 실행합니다. 절 시작 시 먼저 `memory_read`를 호출한 뒤 identity를 확인합니다.

1. `memory_read`로 현재 task와 workspace memory를 읽습니다. 반환 snapshot의 `task` 항목만 검사하고, workspace `storyId` 또는 `storyName`을 현재 task의 기존 identity로 취급하지 않습니다.
2. `task`에 `storyId`와 `storyName`이 모두 있고 trim 후 비어 있지 않을 때만 identity가 완전하다고 보고 원래 역할 Workflow를 계속합니다.
3. key가 없거나 비어 있거나 쌍이 맞지 않으면 요구사항 identity에 대해 정확히 하나의 질문을 먼저 합니다. 필수 의미 선택지는:
   - `不存在`
   - `其他（用户自行输入）`
4. `AskUserQuestion` 또는 동등 elicitation 도구가 있으면 구조화 질문을 사용합니다. 없으면 plain text로 질문하되 위 두 의미 선택지는 유지합니다. 이 질문은 topology 단계, ambiguity round, Interview round, Grill branch가 아닙니다. 사용자 답변 후 원래 Workflow를 계속합니다.
5. 사용자가 `不存在`을 선택하면:
   - `storyId`를 문자열 `0`으로 설정합니다.
   - 요구사항에서 짧은 `storyName`을 추출합니다. 명시 제목 또는 첫 줄 요약을 우선하고, 다음은 goal의 핵심 명사구, 그래도 불가하면 `系统需求`를 사용합니다.
   - 템플릿 라벨, 상태 설명, Markdown marker, 무관 구두점을 제거합니다. 최대 40 Unicode character의 한 줄로 저장합니다.
6. 사용자가 `其他（用户自行输入）`을 선택하면 요구사항 ID와 이름을 모두 얻습니다. free text를 두 값으로 명확히 나눌 수 없으면 한 번 더 질문하고, 여전히 불명확하면 추측하지 말고 사용자 clarification을 기다립니다. 두 값 trim 후 각각 한 줄로 저장합니다.
7. `memory_write`로 두 항목을 task scope에 씁니다. 쓰기 전 최신 `memory_read`의 target key revision을 사용합니다. key가 없으면 `operation="create"`이고 `expectedRevision` 생략. 기존 key를 고치면 `operation="update"`와 matching revision. workspace scope에는 쓰지 않습니다.
8. 두 번 쓴 뒤 `memory_read`를 다시 호출하고 task scope `storyId`와 `storyName`이 사용자 확인 값과 정확히 일치하는지 확인합니다.
9. `gold-band-memory`를 사용할 수 없거나 read가 실패하면 identity를 읽지 못했다고 밝히고 3번에 따라 정상 질문합니다. write 또는 write 후 검증 실패 시 identity가 persist되지 않았다고 밝힙니다. 둘 다 blocker가 아니며 현재 역할 Workflow를 막지 않습니다. 사용자 확인 또는 새로 제공된 `storyId`, `storyName`으로 계속하고, 이후 node가 다시 물을 수 있습니다.
10. memory 파일을 직접 편집하지 않습니다. identity를 얻은 뒤 Interview 역할은 기존 Interview 초기화를, Grill 역할은 기존 decision tree 열거를 계속합니다. tool write가 성공하면 task scope 사본을 유지합니다.
