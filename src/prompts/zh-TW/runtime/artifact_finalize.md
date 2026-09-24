本輪回覆已結束，runtime 尚未收到本節點的 artifact。以下提供或重申輸出協定，不代表任務已經完成，也不要求你提前收尾。請根據你目前的執行意圖，決定是否結束本節點。

- 如果你沒有打算結束本節點，請直接繼續執行，沿用目前任務範圍、工作區和工具權限；無需輸出狀態標籤，也不要為了回應本提示而提前交接。繼續執行後，當你認為本節點已經結束時，再輸出下方 artifact。
- 如果你認為本節點已經結束，請根據已完成的工作輸出下方 artifact。無需重新核對任務目標或驗收要求，也不要因為本提示新增業務工作。
- 輸出 artifact 前，如果目前任務需要報告或其他附件且尚未寫入，將其寫入本次 attempt 的 attachments 目錄；不需要或已經完成則跳過。
{% if can_read_runtime_snapshot %}- 產生 artifact 時，如下方 runtime 上下文明確要求刷新唯讀 runtime 快照，只能讀取其中宣告的快照路徑。
{% endif %}- 繼續執行期間可以正常回覆和使用工具；只有最終輸出 artifact 時，不要附加解釋、Markdown 或程式碼圍欄。
{% if finalize_context %}
以下是只供本次控制結果歸一化使用的 runtime 上下文：
{{ finalize_context }}
{% endif %}

輸出 artifact：{{ artifact }}
輸出類型：{{ kind }}

僅在你認為本節點已經結束、決定輸出 artifact 時，遵守下面協定：
{{ schema }}{% if success_condition %}

runtime 後續會使用以下條件判斷節點結果：
{{ success_condition }}{% endif %}
