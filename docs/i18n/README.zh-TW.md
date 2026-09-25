<div align="center">

<img src="../../web/public/logo.svg" alt="Gold Band" width="128" />

# Gold Band

> 想成為你最後一款 Agent 桌面客戶端
>
> 主流 Agent 客戶端的體驗 + 完整的工作流設計，兼顧日常開發與大型需求的長時間無人值守開發

[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](../../LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#平台與語言)
[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)

[下載](https://github.com/diodeme/Gold-Band/releases) · [線上 UI 預覽](https://gold-band.dion.blue/zh/demo#)

<!-- README-I18N:START -->

[English](../../README.md) | [简体中文](./README.zh-CN.md) | **繁體中文** | [日本語](./README.ja-JP.md) | [한국어](./README.ko-KR.md) | [Português (Brasil)](./README.pt-BR.md) | [Español](./README.es.md)

<!-- README-I18N:END -->

</div>

---

Gold Band 是一款面向本機專案的 AI Agent 桌面客戶端。它透過 Agent Client Protocol（ACP）連接 Claude Code、Codex 等主流 Agent：一套互動設計，多套 harness 隨時切換；同時提供完整的工作流與 AUTO 編排，讓長程任務更穩定、更可觀測，不依賴模型抽獎式分發。

> [!TIP]
> 想先看看長什麼樣子？打開 [線上 UI 預覽](https://gold-band.dion.blue/zh/demo#)，建議使用桌面瀏覽器。線上預覽能力受限，最終效果以客戶端為準。

> [!NOTE]
> Gold Band 仍處於 **Developer Preview**。核心能力已穩定可用，互動細節仍在快速迭代。

## 亮點

- **一個客戶端接入主流 Agent**：內建 Claude Code、Codex、Cursor、Gemini CLI、CodeBuddy、Goose、Qwen Code、OpenCode、Kimi Code、Amp、Pi，也可以自訂接入任何支援 ACP 的 Agent。
- **三種執行模式**：DIRECT 直接對話、WORKFLOW 固定工作流、AUTO 動態編排，涵蓋從日常問答到大型需求的各類任務。
- **工程化管理的工作流**：每個節點可單獨設定 Agent、模型、角色與結果判定方式；支援回到過去的對話進行修復，也支援發起新的 round 繼續實作需求。
- **面向大型任務的 AUTO 模式**：由節點拆分子任務並分派，每個子任務在獨立的 Git worktree 上執行，完成後由 merge 節點合併、accept 節點驗收，再依結果進入下一輪分派。
- **主流 Agent 客戶端能力大致齊全**：SKILL、MCP、角色（Profile）管理，排程任務，需求管理（對接 Multica），檔案檢視與編輯，原始碼管理，內建瀏覽器，IM 遠端介入與通知，以及桌布、頭像、字型、主題等個人化設定。
- **輕量**：基於 Tauri 2 + Rust，安裝包僅數十 MB，多對話並行時記憶體占用約 300 MB。

## 支援的 Agent

| 內建 Agent | |
| --- | --- |
| Claude Code、Codex | 目前推薦的體驗入口 |
| Cursor、Gemini CLI、CodeBuddy、Goose、Qwen Code、OpenCode、Kimi Code、Amp、Pi | 內建接入，可用性取決於本機環境及其 ACP 支援情況 |
| 自訂 Agent | 任何支援 ACP 協定的 Agent 都可以在 Agent 管理中手動接入 |

## 執行模式

### DIRECT

接近直接使用 Agent 本身。Gold Band 不注入工作流 system prompt，只負責統一的桌面 UI、對話保存、附件、模型與權限設定、停止與恢復、Token 與耗時統計。

適合日常問答、程式碼修改、除錯，以及希望保留持續上下文的開發對話。

### WORKFLOW

使用明確的工作流。每個節點代表一次 Agent 執行，可為節點單獨選擇 Agent、模型、角色與結果判定方式；邊定義成功、失敗或人工確認後的流轉方向。執行後可以回到歷史對話修復問題，或發起新的 round 繼續推進。

適合需要明確開發階段、獨立審查與測試、失敗回圈和結構化驗收的任務。

### AUTO / AI-DYNAMIC

由 AI-DYNAMIC 依目標動態提出後續節點：拆分子任務、在各自的 worktree 中並行執行、由 merge 節點合併結果、由 accept 節點驗收，再依目前結果進行新一輪分派。Gold Band runtime 會校驗 proposal 並管理真實執行狀態，Agent 無法直接修改 runtime。

適合難以預先確定完整流程、但仍需要執行邊界與可觀測性的大型或複雜任務。

## 更多能力

- **對話體驗**：串流輸出、繼續追問、歷史恢復、對話複用、可選的外部對話同步；在 composer 中選擇模型、思考等級、權限模式和 Slash Command。
- **附件與產物**：檔案選擇、拖放、圖片貼上、工作區檔案引用、附件預覽和節點產物歸檔。
- **執行觀測**：檢視 Agent 訊息、工具呼叫、系統提示、原始幀、Token、耗時和執行狀態。
- **工作區**：檔案瀏覽與即時編輯、Git 原始碼管理、內建瀏覽器。
- **自動化與協作**：排程任務、Multica 需求管理、IM 遠端介入與通知（目前支援企業微信）、系統通知。
- **Agent 與上下文管理**：統一維護 Agent、Profile、MCP、SKILL 及使用者層級、專案層級上下文，並提供 Agent 環境診斷。
- **個人化**：主題、桌布、字型、使用者與 Agent 自訂頭像，以及個人資料分析。

## 快速開始

1. 從 [Releases](https://github.com/diodeme/Gold-Band/releases) 下載桌面安裝包，或從原始碼建置。
2. 打開 Gold Band，新增一個本機工作區。
3. 在 Agent 管理中啟用 Claude Code、Codex 或其他 ACP Agent，並確認環境診斷通過。
4. 回到對話首頁，選擇執行模式：
   - `DIRECT`：直接與指定 Agent 持續對話，建議首次使用。
   - `WORKFLOW`：使用固定工作流，適合流程明確、需要強驗證的任務。
   - `AUTO`：讓 AI-DYNAMIC 動態拆分與排程，適合開放或複雜目標。
5. 輸入需求，並在對話詳情中檢視輸出、互動請求、附件、產物與執行狀態。

> [!IMPORTANT]
> 專案目前尚未取得 Apple Developer Program 開發者帳號，因此 macOS Release 尚未使用 Developer ID 簽署與 Apple 公證。安裝方式與 Gatekeeper 排錯請參考 [macOS 安裝與排錯指南](../guide/macos-install.zh-CN.md)（簡體中文）。

## 平台與語言

- **平台**：提供 Windows、macOS、Linux 安裝包。目前優先保證 Windows 10 / 11 的使用體驗，其次是 Apple Silicon 與 Intel Mac；Linux 版本尚未完成完整測試。
- **介面語言**：简体中文、繁體中文、English、日本語、한국어、Português (Brasil)、Español。

## 常見問題

### 和 Coding Agent 內部的工作流有什麼差別？

Coding Agent 內部的工作流主要是「主 Agent 編排子 Agent」或「腳本編排 Agent」，編排的是 **session**；Gold Band 編排的是 **harness**。只要支援 ACP，任何 Agent 都可以成為一個節點：例如用內建 browser 與 computer use 能力的 Codex 作為驗收節點，用極簡、快速的 Pi 作為開發節點。節點之間的差異可以是一整套 harness 的差異，而不只是上下文的差異。

### 和 Codex App 這類 Agent 客戶端有什麼差別？

這類客戶端圍繞某一個固定 Agent 建構；Gold Band 位於 Agent 的上層，可以切換並組合不同架構的 Agent，同時繼承它們各自的能力。代價是 Gold Band 無法深入 Agent 內部循環，例如在 Agent loop 中途插入使用者引導這類能力實作成本較高。

### 和其他 ACP 客戶端有什麼差別？

Gold Band 從工作流出發，ACP 客戶端能力是在此基礎上補齊的。工作流不只是簡單排程，還包括驗收標準、前文摘要、停止與繼續、AUTO 模式下的節點歸併等工程能力，由 runtime 統一管理節點推進與失敗處理，Agent 專注於目前節點的任務。

## 目前狀態與規劃

已知問題：

- 互動設計上仍有一些小 Bug，正在持續修復。
- 工作流與 AUTO 模式基於對抗式驗證與 loop 思想，耗時與 Token 消耗會高於直接讓 Agent 做事，但能減少返工。
- 內建終端機與行動端遠端控制尚未提供。

後續規劃：

1. 持續優化客戶端互動體驗，修復已知 UI Bug。
2. 完善工作流的工程化與易用性，例如任意節點重跑、以自然語言建立工作流。
3. 使用工作流與 AUTO 模式完成一個典型的極複雜需求，公開展示編排的有效性。
4. 依 client → p2p / relay → host 架構重構，支援本機與遠端目錄作為工作區，以及多端 client 操控同一個 host。

## 適合與不適合

適合：

- 希望用一個桌面客戶端使用多個本機 Coding Agent。
- 需要持續對話、歷史恢復與附件協作的開發任務。
- 需要把開發、審查、測試與驗收分開的長程任務。
- 希望保留執行過程、產物並支援失敗恢復的任務。

暫不適合：

- 要求穩定商用 SLA 的正式環境。
- 依賴尚未完整支援的 ACP Agent 或 Provider 特性。
- 不願接受 Developer Preview 階段 UI 與行為快速變化的使用者。

## 本機開發

```bash
npm install
npm run dev
```

常用驗證命令：

```bash
cargo check
npm run web:test
npm run web:build
```

## 技術堆疊

- Rust
- React
- Tauri 2
- Tailwind CSS
- shadcn/ui
- prompt-kit
- Agent Client Protocol / ACP

## 社群與回饋

本專案積極參與並支持 [linux.do 社群](https://linux.do)。歡迎 Star、試用，並透過 Issue 與 Pull Request 回饋 Agent 接入、對話體驗、工作流、AUTO 拆解品質及異常恢復問題。

AGPL-3.0-only，詳見 [LICENSE](../../LICENSE)。
