# 本次 AI-DYNAMIC 執行上下文

## 目前動態節點
- 父節點：{{ outer_node_id }}
- 父 attempt：{{ outer_attempt_id }}
- Dynamic run：{{ dynamic_run_id }}
- 內部節點：{{ node_id }}
- 標題：{{ title }}
- 節點類型：{{ kind }}
- 所屬 group：{{ group_id }}
- 所屬 chain：{{ chain_id }}
- 目前深度：{{ depth }}

## 執行位置
- Dynamic 根目錄：{{ dynamic_root }}
- 內部節點（相對 Dynamic 根目錄）：{{ node_dir }}
- 目前 attempt（相對內部節點）：{{ attempt_dir }}
- attachments（相對目前 attempt）：{{ attachments_dir }}
- Workspace ID：{{ workspace_id }}
- Workspace 路徑：{{ workspace_path }}
- Workspace 能力：
{{ workspace_capability }}

{% if has_new_round_trigger %}
## `$new-round` 觸發回饋
{{ new_round_trigger }}
- 這是上一輪觸發目前新 Round 的失敗節點輸出。先理解其中的失敗原因和未完成項，再規劃本輪內部任務，不要只按原始需求原樣重跑。
- artifact 預覽可能被截斷；需要完整資訊時讀取上面明確列出的 artifact 或附件。
{% endif %}

{% if has_coordination_snapshot %}
## Runtime 協調快照
- 唯讀快照（相對 Dynamic 根目錄）：{{ coordination_snapshot_path }}
- 該檔案由 Runtime 從 canonical dynamic graph 產生並獨占寫入；不要修改它。
- 開始或繼續目前任務前讀取最新快照：先按 `workstreams[]` 的目標、TODO 狀態、父子關係與 steps 理解其他子任務，再結合 `groups[]` 的嵌套關係和 phase，避免重複或衝突。
- 準備輸出 `next.type="single"` 或 `next.type="fanout"` 前再次讀取同一路徑，以最新狀態規劃後繼任務。
{% endif %}

{% if has_direct_predecessors %}
## 直接前序節點
{{ direct_predecessors }}
{% endif %}

{% if has_active_group %}
## 目前 group
{{ active_group }}
{% endif %}

{% if has_inherited_groups %}
## 繼承的 group 上下文
{{ inherited_groups }}
{% endif %}

{% if has_siblings %}
## 並行兄弟節點
{{ siblings }}
{% endif %}

{% if has_available_attachments %}
## 可用附件
- 以下只列附件路徑，不讀取或內嵌附件正文。普通條目的完整路徑按 `Dynamic 根目錄` 與路徑樹各層依次拼接；頂層 `absolutePath=` 條目已經是完整路徑，直接使用。
{% if has_predecessor_attachments %}
### 前序鏈路（建立目前節點的任務接力鏈，最多回溯 {{ source_predecessor_limit }} 個節點）
{{ predecessor_attachments }}
{% if has_predecessor_attachment_overflow %}
- 以下來源節點的附件清單已截斷或未完整讀取；每個節點最多檢查 {{ attachments_per_source_limit }} 個檔案或空目錄，含內容的目錄會繼續遞迴且不單獨計數。上方只列找到的檔案，其餘請按需檢視完整 attachments 目錄：
{{ predecessor_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_dependency_attachments %}
### 顯式依賴（目前節點透過 dependsOn 明確指定的輸入節點）
{{ dependency_attachments }}
{% if has_dependency_attachment_overflow %}
- 以下來源節點的附件清單已截斷或未完整讀取；每個節點最多檢查 {{ attachments_per_source_limit }} 個檔案或空目錄，含內容的目錄會繼續遞迴且不單獨計數。上方只列找到的檔案，其餘請按需檢視完整 attachments 目錄：
{{ dependency_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_group_evidence_attachments %}
### Group 證據（目前 merge / acceptance 輸入或相關 group 最近一輪合併與驗收）
{{ group_evidence_attachments }}
{% if has_group_evidence_attachment_overflow %}
- 以下來源節點的附件清單已截斷或未完整讀取；每個節點最多檢查 {{ attachments_per_source_limit }} 個檔案或空目錄，含內容的目錄會繼續遞迴且不單獨計數。上方只列找到的檔案，其餘請按需檢視完整 attachments 目錄：
{{ group_evidence_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% endif %}

{% if has_output_contract %}
## 會話複用
- Session mode：{{ session_mode }}
- continueFromNodeId：{{ continue_from_node_id }}
- 說明：`continue` 只表示複用來源節點的 ACP session 上下文；目前任務以本次 user prompt 的 `# 任務` 為準。
- 目前鏈路可複用會話節點：
{{ resumable_sessions }}

## 執行預算
- Allowed workflow snapshots：
{{ allowed_workflow_snapshots }}
- 剩餘預算：
{{ remaining_budget }}

## Agent 與 profile 選項
- 動態節點 agent 策略：{{ agent_strategy_mode }}
- 初始分發節點 agent：{{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Agent 決策指南：
{{ agent_routing_prompt }}
- merge / acceptance 模型策略：
{{ acceptance_model_policy }}
{% endif %}- 可用 agent 及預配執行參數：
{{ available_providers }}
- 可用 profiles：
{{ available_profiles }}
{% endif %}
