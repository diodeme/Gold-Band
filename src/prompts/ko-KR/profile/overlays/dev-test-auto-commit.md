## 개발·테스트 자동 커밋

요구사항 구현과 필요 자동화 test 완료 후, node 종료 전 이 절을 실행합니다.

1. `memory_read`로 task scope `storyId`, `storyName`을 확인합니다. 둘 다 비어 있지 않으면 재사용합니다. 하나라도 없거나 비어 있거나 쌍이 맞지 않으면 `storyId=0`과 요구사항 identity 추출 규칙으로 생성하고 `memory_write`를 시도합니다. 없는 key는 `operation="create"`와 `expectedRevision` 생략, 기존 key는 `operation="update"`와 revision. tool 불가 또는 read 실패 시 identity를 읽지 못했다고 밝힙니다. write 또는 검증 실패 시 persist되지 않았다고 밝힙니다. 둘 다 blocker가 아니며 사용자에게 identity를 묻거나 이번 run에서 확인한 identity로 계속할 수 있습니다.
2. 이번 node가 실제 만든 task 관련 변경을 식별합니다. code, test, prompt, product-design 문서, development plan 포함. 실행 전 존재하거나 명백히 무관한 사용자 변경은 제외합니다.
3. task 관련 변경이 없으면 empty commit을 만들지 않습니다. "커밋 불필요", 근거, 포함하지 않은 uncommitted 변경을 기록합니다.
4. task 관련 diff에서 표준 Conventional Commits type token과 간결한 한국어 설명을 선택합니다. 실행 과정·모델 이름·일반 설명이 아니라 전달 결과에 초점을 맞춥니다.
5. `git add`는 구체 path만 사용합니다. `git add -A` 금지, 무관 변경 포함 금지. node 성공 실행마다 논리 commit 하나.
6. commit message는 정확히 세 줄:

```text
--story=[{storyId}] {storyName}
{type}: {中文描述}
#AI COMMIT#
```

7. commit 후 commit OID, 전체 commit message, task 관련 path 상태를 검증합니다. commit 또는 검증 실패 시 node 완료를 주장하지 마십시오.
8. development-and-testing report에 commit OID, 세 줄 message, 포함 path, 제외 path, 사유를 기록합니다. CI/CD는 이 commit OID로 unpushed commit 여부를 판단합니다.
9. commit message나 형식 확인을 사용자에게 요청하지 않습니다. 기존 ACP command-permission 규칙은 유지하며 auto-commit을 위해 permission 경계를 우회하지 마십시오.
10. commit 실패 시 원래 error와 현재 Git 상태를 보존하고 기존 node-failure, manual-recovery, acceptance-failure 경로에 맡깁니다. 대체 commit이나 history rewrite를 하지 마십시오.
