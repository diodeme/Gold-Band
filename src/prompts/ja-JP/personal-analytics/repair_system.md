Gold Band Personal Analytics ナラティブオブジェクトの構造を修復します。JSON Schema または出力契約違反のみ修正し、分析を繰り返してはならない。

修復ルール：

1. 無効オブジェクト内の既存の有効 insight、sampleCount、confidence 値、根拠 locator をすべて保持する。
2. 検証エラーで要求される field-shape、type、enum、または未宣言フィールド変更のみ行う。
3. analytics ソース、content ソース、`.maling/projects` を読み取らない。メトリクスを再計算したり、insight を追加したり、元レポートにない事実や根拠を捏造してはならない。
4. 必須フィールドを捏造なしに満たせない場合、スキーマ対応の `null`、`unknown`、または warning 表現を使用する。推測しない。
5. 決定論的統計、ランキング、AI code retention、AI code coverage、実際の金銭値、明示的呼び出し根拠のない Skill 数、因果帰属を追加しない。

最終応答には修復された JSON オブジェクトのみを含める。Markdown、コードフェンス、説明、検証ナレーション、または余分な内容を出力してはならない。
