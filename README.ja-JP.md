<div align="center">

<img src="web/public/logo.svg" alt="Gold Band" width="128" />

# Gold Band

> あなたにとって最後の Agent デスクトップクライアントを目指して
>
> 主要な Agent クライアントの使い心地と本格的なワークフロー設計で、日常の開発から大規模要件の長時間無人開発までカバー

[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#プラットフォームと言語)
[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)

[ダウンロード](https://github.com/diodeme/Gold-Band/releases) · [オンライン UI プレビュー](https://gold-band.dion.blue/en/demo#)

<!-- README-I18N:START -->

[English](./README.md) | [简体中文](./README.zh-CN.md) | [繁體中文](./README.zh-TW.md) | **日本語** | [한국어](./README.ko-KR.md) | [Português (Brasil)](./README.pt-BR.md) | [Español](./README.es.md)

<!-- README-I18N:END -->

</div>

---

Gold Band は、ローカルプロジェクト向けの AI Agent デスクトップクライアントです。Agent Client Protocol（ACP）を通じて Claude Code や Codex などの主要な Agent に接続します。ひとつの操作体系で、複数の harness を自由に切り替えられます。さらに本格的なワークフローと AUTO オーケストレーションを備え、長時間のタスクでも安定性と可観測性を保ち、モデルの一発勝負に頼りません。

> [!TIP]
> まずは見た目を確認したい場合は、[オンライン UI プレビュー](https://gold-band.dion.blue/en/demo#) をデスクトップブラウザで開いてください。オンライン版は機能が制限されており、最終的な体験はデスクトップクライアントが基準です。

> [!NOTE]
> Gold Band は現在 **Developer Preview** です。コア機能は安定していますが、操作の細部は急速に改善中です。

## 特長

- **ひとつのクライアントで主要な Agent に対応**：Claude Code、Codex、Cursor、Gemini CLI、CodeBuddy、Goose、Qwen Code、OpenCode、Kimi Code、Amp、Pi を内蔵し、ACP 対応の任意の Agent も独自に追加できます。
- **3 つの実行モード**：DIRECT（直接会話）、WORKFLOW（固定ワークフロー）、AUTO（動的オーケストレーション）で、簡単な質問から大規模な要件まで対応します。
- **エンジニアリングされたワークフロー**：ノードごとに Agent、モデル、ロール、結果判定方法を設定できます。過去のセッションに戻って修正したり、新しい round を開始して実装を続けたりできます。
- **大規模タスク向けの AUTO モード**：ノードが目標をサブタスクに分割して配布し、各サブタスクは独立した Git worktree で実行されます。完了後は merge ノードが統合し、accept ノードが検収し、その結果に基づいて次のラウンドを配布します。
- **Agent クライアントに期待される機能をひととおり搭載**：SKILL、MCP、ロール（Profile）管理、スケジュールタスク、要件管理（Multica 連携）、ファイルの閲覧と編集、ソース管理、内蔵ブラウザ、IM によるリモート介入と通知、壁紙・アバター・フォント・テーマなどのカスタマイズ。
- **軽量**：Tauri 2 と Rust で開発。インストーラーは数十 MB 程度で、複数セッションを並行実行してもメモリ使用量は約 300 MB です。

## 対応 Agent

| 内蔵 Agent | |
| --- | --- |
| Claude Code、Codex | 現在のおすすめ |
| Cursor、Gemini CLI、CodeBuddy、Goose、Qwen Code、OpenCode、Kimi Code、Amp、Pi | 内蔵済み。利用可否はローカル環境と各 Agent の ACP 対応状況によります |
| カスタム Agent | ACP 対応の Agent であれば Agent 管理から手動で追加できます |

## 実行モード

### DIRECT

Agent をそのまま使う感覚に近いモードです。Gold Band はワークフロー用の system prompt を注入せず、統一されたデスクトップ UI、セッション保存、添付ファイル、モデルと権限の設定、停止と復元、Token と所要時間の計測だけを提供します。

日常的な質問、コード修正、デバッグ、継続的なコンテキストを保ちたい開発セッションに適しています。

### WORKFLOW

明示的なワークフローを使用します。各ノードは 1 回の Agent 実行を表し、ノードごとに Agent、モデル、ロール、結果判定方法を選べます。エッジは成功、失敗、手動確認後の遷移先を定義します。実行後は過去のセッションに戻って問題を修正したり、新しい round を開始して先に進めたりできます。

明確な開発フェーズ、独立したレビューとテスト、失敗時のループ、構造化された検収が必要なタスクに適しています。

### AUTO / AI-DYNAMIC

AI-DYNAMIC が目標に応じて次のノードを動的に提案します。サブタスクを分割し、それぞれの worktree で並行実行し、merge ノードで結果を統合し、accept ノードで検収したうえで、現在の結果に基づいて新しいラウンドを配布します。Gold Band runtime が proposal を検証し、実際の実行状態を管理します。Agent が runtime を直接変更することはできません。

全体の流れを事前に決めにくい一方で、実行の境界と可観測性が求められる大規模・複雑なタスクに適しています。

## その他の機能

- **会話**：ストリーミング出力、追加の質問、履歴の復元、セッションの再利用、外部セッション同期（任意）。composer からモデル、思考レベル、権限モード、Slash Command を選択できます。
- **添付ファイルと成果物**：ファイル選択、ドラッグ＆ドロップ、画像の貼り付け、ワークスペースファイルの参照、プレビュー、ノード成果物のアーカイブ。
- **実行の可観測性**：Agent メッセージ、ツール呼び出し、システムプロンプト、生フレーム、Token、所要時間、実行状態を確認できます。
- **ワークスペース**：ファイルの閲覧とリアルタイム編集、Git によるソース管理、内蔵ブラウザ。
- **自動化とコラボレーション**：スケジュールタスク、Multica による要件管理、IM によるリモート介入と通知（現在は WeCom に対応）、システム通知。
- **Agent とコンテキストの管理**：Agent、Profile、MCP、SKILL、ユーザー単位・プロジェクト単位のコンテキストを一元管理し、Agent の環境診断も提供します。
- **カスタマイズ**：テーマ、壁紙、フォント、ユーザーと Agent のアバター、個人の利用状況分析。

## クイックスタート

1. [Releases](https://github.com/diodeme/Gold-Band/releases) からデスクトップ版をダウンロードするか、ソースからビルドします。
2. Gold Band を開き、ローカルワークスペースを追加します。
3. Agent 管理で Claude Code、Codex、またはその他の ACP Agent を有効にし、環境診断が通ることを確認します。
4. 会話ホームに戻り、実行モードを選択します：
   - `DIRECT`：選択した Agent と継続的に会話します。初めての方におすすめです。
   - `WORKFLOW`：固定ワークフローを使います。フェーズが明確で強い検証が必要なタスク向けです。
   - `AUTO`：AI-DYNAMIC に動的な分割とスケジューリングを任せます。オープンな目標や複雑な目標向けです。
5. 要件を入力し、会話の詳細画面で出力、インタラクション要求、添付ファイル、成果物、実行状態を確認します。

> [!IMPORTANT]
> 本プロジェクトはまだ Apple Developer Program のアカウントを取得していないため、macOS 版は Developer ID による署名と Apple の公証を受けていません。インストール方法と Gatekeeper のトラブルシューティングは [macOS Installation and Troubleshooting Guide](docs/guide/macos-install.md)（英語）を参照してください。

## プラットフォームと言語

- **プラットフォーム**：Windows、macOS、Linux 向けのパッケージを提供しています。現在は Windows 10 / 11 の体験を最優先し、次いで Apple Silicon と Intel Mac に対応しています。Linux 版はまだ十分にテストされていません。
- **UI 言語**：简体中文、繁體中文、English、日本語、한국어、Português (Brasil)、Español。

## よくある質問

### Coding Agent 内部のワークフローとの違いは？

Coding Agent 内部のワークフローは主に「メイン Agent がサブ Agent を編成する」または「スクリプトが Agent を編成する」もので、編成の対象は **session** です。Gold Band が編成するのは **harness** です。ACP に対応していれば、どの Agent もノードになれます。たとえば、内蔵の browser と computer use 機能を持つ Codex を検収ノードに、ミニマルで高速な Pi を開発ノードに使えます。ノード間の違いはコンテキストだけでなく、harness 全体の違いにもなり得ます。

### Codex App のような Agent クライアントとの違いは？

そうしたクライアントは特定の Agent を中心に作られています。Gold Band は Agent の上位レイヤーに位置し、異なるアーキテクチャの Agent を切り替えたり組み合わせたりしながら、それぞれの機能を活用できます。その代わり、Agent 内部のループには介入できないため、ループの途中でユーザーの指示によって方向を修正するような機能は実現コストが高くなります。

### 他の ACP クライアントとの違いは？

Gold Band はワークフローを出発点とし、その上に ACP クライアント機能を整えてきました。ワークフローは単純なスケジューリングにとどまらず、検収基準、前段の要約、停止と再開、AUTO モードでのノード統合といったエンジニアリング機能を含みます。runtime がノードの進行と失敗処理を管理するため、各 Agent は目の前のノードのタスクに集中できます。

## 現状とロードマップ

既知の課題：

- 操作面にまだ小さなバグがあり、継続的に修正しています。
- WORKFLOW と AUTO は敵対的検証とループの考え方に基づくため、Agent に直接頼むよりも時間と Token を多く消費しますが、手戻りを減らせます。
- 内蔵ターミナルとモバイルからのリモート操作はまだ提供していません。

今後の予定：

1. クライアントの操作体験を改善し、既知の UI バグを修正します。
2. ワークフローの堅牢性と使いやすさを高めます（任意ノードの再実行、自然言語によるワークフロー作成など）。
3. WORKFLOW と AUTO で典型的な超複雑要件を実装し、オーケストレーションの有効性を公開で示します。
4. client → p2p / relay → host アーキテクチャへ再構築し、ローカルとリモートのディレクトリをワークスペースとして扱えるようにし、複数のクライアントから 1 つの host を操作できるようにします。

## 向いている用途

向いている用途：

- 複数のローカル Coding Agent をひとつのデスクトップクライアントで使いたい。
- 継続的な会話、履歴の復元、添付ファイルでの共同作業が必要な開発タスク。
- 開発、レビュー、テスト、検収を分けて進める長期タスク。
- 実行過程や成果物を記録し、失敗から復旧したいタスク。

まだ向いていない用途：

- 安定した商用 SLA が求められる本番環境。
- まだ完全には対応していない ACP Agent や Provider の機能に依存するワークロード。
- Developer Preview 期間中の UI や挙動の急速な変化を望まないユーザー。

## ローカル開発

```bash
npm install
npm run dev
```

よく使う検証コマンド：

```bash
cargo check
npm run web:test
npm run web:build
```

## 技術スタック

- Rust
- React
- Tauri 2
- Tailwind CSS
- shadcn/ui
- prompt-kit
- Agent Client Protocol / ACP

## コミュニティとフィードバック

本プロジェクトは [linux.do コミュニティ](https://linux.do) に積極的に参加し、支援しています。Star、試用、そして Agent 連携、会話体験、ワークフロー、AUTO の分割品質、エラー復旧に関する Issue や Pull Request を歓迎します。

AGPL-3.0-only。詳細は [LICENSE](LICENSE) を参照してください。
