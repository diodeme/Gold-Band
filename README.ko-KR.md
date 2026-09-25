<div align="center">

<img src="web/public/logo.svg" alt="Gold Band" width="128" />

# Gold Band

> 당신의 마지막 Agent 데스크톱 클라이언트를 목표로
>
> 주요 Agent 클라이언트의 사용 경험과 완전한 워크플로 설계로, 일상 개발부터 대규모 요구 사항의 장시간 무인 개발까지 지원

[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#플랫폼과-언어)
[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)

[다운로드](https://github.com/diodeme/Gold-Band/releases) · [온라인 UI 미리보기](https://gold-band.dion.blue/en/demo#)

<!-- README-I18N:START -->

[English](./README.md) | [简体中文](./README.zh-CN.md) | [繁體中文](./README.zh-TW.md) | [日本語](./README.ja-JP.md) | **한국어** | [Português (Brasil)](./README.pt-BR.md) | [Español](./README.es.md)

<!-- README-I18N:END -->

</div>

---

Gold Band는 로컬 프로젝트를 위한 AI Agent 데스크톱 클라이언트입니다. Agent Client Protocol(ACP)을 통해 Claude Code, Codex 등 주요 Agent에 연결합니다. 하나의 인터랙션 설계로 여러 harness를 자유롭게 전환할 수 있습니다. 또한 완전한 워크플로와 AUTO 오케스트레이션을 제공하여 장시간 작업도 안정적이고 관측 가능하게 유지하며, 모델의 한 번의 운에 기대지 않습니다.

> [!TIP]
> 먼저 어떤 모습인지 보고 싶다면 [온라인 UI 미리보기](https://gold-band.dion.blue/en/demo#)를 데스크톱 브라우저에서 열어 보세요. 온라인 미리보기는 기능이 제한되어 있으며, 최종 경험은 데스크톱 클라이언트를 기준으로 합니다.

> [!NOTE]
> Gold Band는 아직 **Developer Preview** 단계입니다. 핵심 기능은 안정적이지만 인터랙션 세부 사항은 빠르게 개선되고 있습니다.

## 주요 특징

- **하나의 클라이언트로 주요 Agent 지원**: Claude Code, Codex, Cursor, Gemini CLI, CodeBuddy, Goose, Qwen Code, OpenCode, Kimi Code, Amp, Pi를 기본 제공하며, ACP를 지원하는 모든 Agent를 직접 추가할 수 있습니다.
- **세 가지 실행 모드**: DIRECT 직접 대화, WORKFLOW 고정 워크플로, AUTO 동적 오케스트레이션으로 간단한 질문부터 대규모 요구 사항까지 처리합니다.
- **엔지니어링된 워크플로**: 노드마다 Agent, 모델, 역할, 결과 판정 방식을 따로 설정할 수 있습니다. 이전 세션으로 돌아가 수정하거나 새 round를 시작해 요구 사항 구현을 이어갈 수 있습니다.
- **대규모 작업을 위한 AUTO 모드**: 노드가 목표를 하위 작업으로 나누어 배분하고, 각 하위 작업은 독립된 Git worktree에서 실행됩니다. 완료 후 merge 노드가 병합하고 accept 노드가 검수하며, 그 결과에 따라 다음 라운드를 배분합니다.
- **Agent 클라이언트에 기대하는 기능을 대부분 제공**: SKILL, MCP, 역할(Profile) 관리, 예약 작업, 요구 사항 관리(Multica 연동), 파일 보기 및 편집, 소스 관리, 내장 브라우저, IM 원격 개입 및 알림, 배경화면·아바타·글꼴·테마 등 개인화 설정.
- **가벼움**: Tauri 2와 Rust로 개발되어 설치 파일은 수십 MB에 불과하며, 여러 세션을 병렬로 실행해도 메모리 사용량은 약 300 MB입니다.

## 지원 Agent

| 기본 제공 Agent | |
| --- | --- |
| Claude Code, Codex | 현재 추천하는 시작점 |
| Cursor, Gemini CLI, CodeBuddy, Goose, Qwen Code, OpenCode, Kimi Code, Amp, Pi | 기본 제공되며, 사용 가능 여부는 로컬 환경과 각 Agent의 ACP 지원 상황에 따라 다릅니다 |
| 사용자 정의 Agent | ACP를 지원하는 모든 Agent를 Agent 관리에서 직접 추가할 수 있습니다 |

## 실행 모드

### DIRECT

Agent를 직접 사용하는 것과 비슷합니다. Gold Band는 워크플로 system prompt를 주입하지 않고, 통합 데스크톱 UI, 세션 저장, 첨부 파일, 모델 및 권한 설정, 중지와 복구, Token 및 소요 시간 통계만 제공합니다.

일상적인 질문, 코드 수정, 디버깅, 지속적인 컨텍스트가 필요한 개발 세션에 적합합니다.

### WORKFLOW

명시적인 워크플로를 사용합니다. 각 노드는 한 번의 Agent 실행을 나타내며 노드마다 Agent, 모델, 역할, 결과 판정 방식을 선택할 수 있습니다. 엣지는 성공, 실패, 수동 확인 이후의 전환 방향을 정의합니다. 실행 후에는 이전 세션으로 돌아가 문제를 수정하거나 새 round를 시작해 계속 진행할 수 있습니다.

명확한 개발 단계, 독립적인 리뷰와 테스트, 실패 루프, 구조화된 검수가 필요한 작업에 적합합니다.

### AUTO / AI-DYNAMIC

AI-DYNAMIC이 목표에 따라 다음 노드를 동적으로 제안합니다. 하위 작업을 나누어 각자의 worktree에서 병렬로 실행하고, merge 노드로 결과를 병합하며, accept 노드로 검수한 뒤 현재 결과를 바탕으로 새 라운드를 배분합니다. Gold Band runtime은 proposal을 검증하고 실제 실행 상태를 관리하며, Agent는 runtime을 직접 변경할 수 없습니다.

전체 흐름을 미리 정하기 어렵지만 실행 경계와 관측 가능성이 필요한 대규모 또는 복잡한 작업에 적합합니다.

## 추가 기능

- **대화**: 스트리밍 출력, 후속 질문, 기록 복구, 세션 재사용, 선택적 외부 세션 동기화. composer에서 모델, 사고 수준, 권한 모드, Slash Command를 선택할 수 있습니다.
- **첨부 파일과 산출물**: 파일 선택, 드래그 앤 드롭, 이미지 붙여넣기, 워크스페이스 파일 참조, 미리보기, 노드 산출물 보관.
- **실행 관측**: Agent 메시지, 도구 호출, 시스템 프롬프트, 원시 프레임, Token, 소요 시간, 실행 상태를 확인할 수 있습니다.
- **워크스페이스**: 파일 탐색과 실시간 편집, Git 소스 관리, 내장 브라우저.
- **자동화와 협업**: 예약 작업, Multica 요구 사항 관리, IM 원격 개입 및 알림(현재 WeCom 지원), 시스템 알림.
- **Agent와 컨텍스트 관리**: Agent, Profile, MCP, SKILL, 사용자 수준 및 프로젝트 수준 컨텍스트를 통합 관리하고 Agent 환경 진단을 제공합니다.
- **개인화**: 테마, 배경화면, 글꼴, 사용자 및 Agent 아바타, 개인 사용 데이터 분석.

## 빠른 시작

1. [Releases](https://github.com/diodeme/Gold-Band/releases)에서 데스크톱 설치 파일을 내려받거나 소스에서 빌드합니다.
2. Gold Band를 열고 로컬 워크스페이스를 추가합니다.
3. Agent 관리에서 Claude Code, Codex 또는 다른 ACP Agent를 활성화하고 환경 진단이 통과하는지 확인합니다.
4. 대화 홈으로 돌아가 실행 모드를 선택합니다:
   - `DIRECT`: 선택한 Agent와 계속 대화합니다. 처음 사용할 때 추천합니다.
   - `WORKFLOW`: 고정 워크플로를 사용합니다. 단계가 명확하고 강한 검증이 필요한 작업에 적합합니다.
   - `AUTO`: AI-DYNAMIC이 동적으로 분할하고 스케줄링합니다. 열린 목표나 복잡한 목표에 적합합니다.
5. 요구 사항을 입력하고 대화 상세 화면에서 출력, 상호작용 요청, 첨부 파일, 산출물, 실행 상태를 확인합니다.

> [!IMPORTANT]
> 이 프로젝트는 아직 Apple Developer Program 계정이 없어 macOS 릴리스가 Developer ID로 서명되거나 Apple의 공증을 받지 않았습니다. 설치 방법과 Gatekeeper 문제 해결은 [macOS Installation and Troubleshooting Guide](docs/guide/macos-install.md)(영어)를 참고하세요.

## 플랫폼과 언어

- **플랫폼**: Windows, macOS, Linux 설치 파일을 제공합니다. 현재 Windows 10 / 11 경험을 가장 우선하며, 그다음은 Apple Silicon 및 Intel Mac입니다. Linux 버전은 아직 충분히 테스트되지 않았습니다.
- **UI 언어**: 简体中文, 繁體中文, English, 日本語, 한국어, Português (Brasil), Español.

## 자주 묻는 질문

### Coding Agent 내부의 워크플로와 무엇이 다른가요?

Coding Agent 내부의 워크플로는 주로 "메인 Agent가 하위 Agent를 조율"하거나 "스크립트가 Agent를 조율"하는 방식으로, **session**을 조율합니다. Gold Band는 **harness**를 조율합니다. ACP를 지원한다면 어떤 Agent든 노드가 될 수 있습니다. 예를 들어 내장 browser와 computer use 기능을 갖춘 Codex를 검수 노드로, 미니멀하고 빠른 Pi를 개발 노드로 사용할 수 있습니다. 노드 간의 차이는 컨텍스트뿐 아니라 harness 전체의 차이가 될 수 있습니다.

### Codex App 같은 Agent 클라이언트와 무엇이 다른가요?

그런 클라이언트는 하나의 고정된 Agent를 중심으로 만들어졌습니다. Gold Band는 Agent보다 상위 계층에 있어 서로 다른 구조의 Agent를 전환하고 조합하면서 각 Agent의 기능을 그대로 활용할 수 있습니다. 대신 Agent 내부 루프에는 개입할 수 없으므로, 루프 도중 사용자 프롬프트로 방향을 조정하는 기능 같은 것은 구현 비용이 더 큽니다.

### 다른 ACP 클라이언트와 무엇이 다른가요?

Gold Band는 워크플로에서 출발했고, ACP 클라이언트 기능은 그 위에 갖춰졌습니다. 워크플로는 단순한 스케줄링을 넘어 검수 기준, 이전 컨텍스트 요약, 중지와 재개, AUTO 모드의 노드 병합 같은 엔지니어링 기능을 포함합니다. runtime이 노드 진행과 실패 처리를 관리하므로 각 Agent는 현재 노드의 작업에 집중할 수 있습니다.

## 현재 상태와 로드맵

알려진 문제:

- 인터랙션에 아직 작은 버그가 있으며 계속 수정하고 있습니다.
- WORKFLOW와 AUTO는 적대적 검증과 루프 방식을 기반으로 하므로 Agent에게 직접 맡기는 것보다 시간과 Token이 더 들지만 재작업을 줄여 줍니다.
- 내장 터미널과 모바일 원격 제어는 아직 제공되지 않습니다.

로드맵:

1. 클라이언트 사용 경험을 계속 개선하고 알려진 UI 버그를 수정합니다.
2. 임의 노드 재실행, 자연어로 워크플로 만들기 등 워크플로의 견고성과 사용성을 높입니다.
3. WORKFLOW와 AUTO로 대표적인 초고난도 요구 사항을 구현해 오케스트레이션의 효과를 공개적으로 보여 줍니다.
4. client → p2p / relay → host 아키텍처로 재구성하여 로컬 및 원격 디렉터리를 워크스페이스로 사용하고, 여러 클라이언트가 하나의 host를 제어할 수 있도록 합니다.

## 적합한 경우

적합한 경우:

- 여러 로컬 Coding Agent를 하나의 데스크톱 클라이언트로 사용하고 싶은 경우.
- 지속적인 대화, 기록 복구, 첨부 파일 협업이 필요한 개발 작업.
- 개발, 리뷰, 테스트, 검수를 분리해 진행하는 장기 작업.
- 실행 과정과 산출물을 기록하고 실패에서 복구해야 하는 작업.

아직 적합하지 않은 경우:

- 안정적인 상용 SLA가 필요한 운영 환경.
- 아직 완전히 지원되지 않는 ACP Agent 또는 Provider 기능에 의존하는 작업.
- Developer Preview 단계의 빠른 UI 및 동작 변화를 원하지 않는 사용자.

## 로컬 개발

```bash
npm install
npm run dev
```

자주 쓰는 검증 명령:

```bash
cargo check
npm run web:test
npm run web:build
```

## 기술 스택

- Rust
- React
- Tauri 2
- Tailwind CSS
- shadcn/ui
- prompt-kit
- Agent Client Protocol / ACP

## 커뮤니티와 피드백

이 프로젝트는 [linux.do 커뮤니티](https://linux.do)에 적극적으로 참여하고 이를 지원합니다. Star와 사용 후기, 그리고 Agent 연동, 대화 경험, 워크플로, AUTO 분할 품질, 오류 복구에 관한 Issue와 Pull Request를 환영합니다.

AGPL-3.0-only. 자세한 내용은 [LICENSE](LICENSE)를 참고하세요.
