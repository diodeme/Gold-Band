你必須在最後一步只輸出 `dynamic-node-completion` artifact 對應的 JSON 內容，不要輸出解釋、Markdown、程式碼圍欄或額外文字。

{% if agent_strategy_mode == "fixed" %}
目前 AI-DYNAMIC 使用固定 agent 策略：除 `workflow-invocation` 外，所有 internal worker、merge、acceptance 節點都會由 runtime 自動使用同一個固定 provider。你不需要為任何節點輸出 provider，輸出中也不要包含 provider 欄位。
{{ model_policy }}
{% else %}
目前 AI-DYNAMIC 使用動態 agent 策略：你只需要根據目前 prompt 裡的「節點 agent 選擇說明」和「可用 providers」，為後續 worker 明確輸出 provider。merge / acceptance 固定由初始分發 Agent 執行，不要為它們輸出 provider。任何節點都不要輸出 `model` 或 `permissionMode`；runtime 會讀取預設定。
{{ model_policy }}
{% endif %}

下面的 JSON Schema 是本次執行的有效輸出協定，由 runtime 從 Rust 資料結構產生並按目前 AI-DYNAMIC 設定動態收窄。你的輸出必須滿足它；runtime 也會使用同一份 schema 做校驗和 repair 診斷。

```json
{{ json_schema }}
```

約束提醒：
- 後繼任務只能分解既定範圍內結果或修復合格 `BLOCKER`；不得把 `FOLLOW_UP` 或前序建議升級為新結果。範圍漂移只安排恢復最小範圍內方案。
{% if agent_strategy_mode == "fixed" %}- 固定 agent 策略下，不要輸出任何 `provider` 欄位；runtime 會自動填充固定 agent。
{% else %}- 動態 agent 策略下，worker 必須輸出合法 provider，且必須符合目前 prompt 給出的節點 agent 選擇說明；`merge / acceptance` 不要輸出 provider，runtime 會固定使用初始分發 Agent。
- `workflow-invocation` 不要輸出 `provider`。
{% endif %}- {{ model_policy }}
- `next.type="end"` 時，`next` 中不要再放 `node / groupId / nodes / merge / acceptance`。
{% if end_summary_is_outer_handoff %}- 如果本次使用 `next.type="end"`，`summary` 必須是交給 AI-DYNAMIC 外層後繼節點的完整業務交接摘要：說明已完成內容、關鍵結論、重要產物及仍需關注事項；不要只寫路由動作或「驗收通過」。
{% else %}- 如果本次使用 `next.type="end"`，`summary` 是內部進度/分支報告，準確說明本節點完成內容，供 Runtime 報告清單和上層 group 使用。
{% endif %}
- `next.type="single"` 時，必須提供完整的 `next.node`，不要提供 `groupId / nodes / merge / acceptance`。
- 不要為任何節點輸出 `workspace`、workspace mode、路徑或分支；runtime 獨占工作空間分配權。
- `next.type="single"` 會自動繼承目前節點的實際 workspace。
- 目前節點若為 group acceptance，合法輸出被接受後該 group 關閉：`single` 接回父作用域的原業務分支；`fanout` 在父作用域建立新 group；只有 `end` 才結束該分支。有後繼時父 group 繼續等待，修復和複驗必須顯式安排，舊 group 不自動重開。
- `next.type="fanout"` 時，必須同時提供 `groupId / nodes / merge / acceptance`，且 `nodes` 至少包含兩個分支；只有一個後繼節點時使用 `next.type="single"`。
- `next.type="fanout"` 的每個 child 會自動獲得隔離 worktree；merge 與 acceptance 自動回到該 group 的父 workspace。
- fanout 的子 worktree 只繼承同一個已提交的 commit，不繼承未提交內容；Runtime 不會自動 checkpoint。若本次任務存在後續分支需要、但尚未提交的業務改動，請審閱後按具體路徑提交，可使用 Conventional Commits；沒有需要提交的改動則不進行 Git 操作。
- 工作區乾淨不是 fanout 門禁。Runtime 首次偵測到髒檔案時只提醒一次，之後重新輸出 artifact 即可，不要求產生新 commit。不要為此清理工作區、stash 無關內容、移動其他 worktree、盲目 `git add -A` 或改變忽略規則；不屬於本次交付的內容保持原樣。
- `profile` 只允許在 worker 節點中使用，選填；如果填寫，必須使用 schema enum 或目前 prompt 中 `profileId=...` 後面的 ID，不要填寫 displayName。
- `merge` / `acceptance` 不要輸出 `profile`；它們統一使用 runtime 內建的 AI-DYNAMIC merge / acceptance prompt。
{% if agent_strategy_mode == "dynamic" %}- `provider` 如果填寫，必須是 schema enum 或目前 prompt 中列出的可用 provider 之一。
{% endif %}- `sessionMode` 不填時按 `new` 處理；只有要繼續目前鏈路內可複用會話節點時才填 `continue`。
- `sessionMode="continue"` 時必須填寫 `continueFromNodeId`，且只能引用目前 prompt 列出的可複用會話節點。
- `workflow-invocation` 不要使用 `sessionMode="continue"`。
- `workflowId` 如果填寫，必須是 schema enum 或目前 prompt 中列出的 allowed workflow DSL ID 之一。
- fanout 的節點數量必須滿足 schema `minItems/maxItems`、目前 prompt 給出的 `maxFanout` 和剩餘預算約束。
- 不要輸出偽程式碼、說明文字或範例包裹語；只輸出最終 JSON。
