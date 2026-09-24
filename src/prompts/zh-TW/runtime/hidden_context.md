# 本次 Gold Band 執行上下文

- 會話模式: {{ session_mode }}
- Round: {{ round_id }}
- Attempt: {{ attempt_id }}
- Attempt 目錄: {{ attempt_dir }}
- 附件目錄（本節點報告、暫時腳本、過程記錄等自由輸出預設寫入這裡）: {{ attachments_dir }}
{% if invocation_reason %}
- 呼叫原因: {{ invocation_reason }}
{% endif %}

{% if predecessors.is_empty %}
## 最新前序執行鏈
目前節點的前序執行節點：無，目前節點是本輪入口節點。
{% else %}
## 最新前序執行鏈
{{ predecessors.chain }}
{% endif %}

{% if predecessors.reason_lines_empty %}
{% if predecessors.is_empty %}
## 最新前序流轉原因
無。
{% else %}
## 最新前序流轉原因
前序節點均為普通節點，按節點結果進入目前分支。
{% endif %}
{% else %}
## 最新前序流轉原因
{{ predecessors.reason_lines }}
{% endif %}

{% if predecessors.has_ai_dynamic_report_manifest %}
## AI-DYNAMIC 完整報告清單（按需讀取）
前序 `ai-dynamic-result` 中的 `reportManifest.path` 指向完整內部執行報告索引，包含節點/group 拓撲、依賴與時間關係、workspace、內部 summary 和附件位址。預設使用業務交接 `summary`；僅當需要核對內部過程、查找報告附件，或 `summary` 資訊不足時，才讀取該清單。
{% endif %}

{% if not predecessors.attachment_lines_empty %}
## 最新前序附件
{{ predecessors.attachment_lines }}
{% endif %}
