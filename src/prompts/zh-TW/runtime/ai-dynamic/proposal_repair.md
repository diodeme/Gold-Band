上一輪 `dynamic-node-completion` proposal 尚未接受，請處理下列校驗項或提醒後重新輸出。

你必須修復最終的 `dynamic-node-completion` 輸出，使其滿足下面這些 runtime 約束。
只修復協定校驗錯誤，不重新執行任務；後繼任務仍須符合範圍契約，越界項只刪除或收窄。
{% if fanout_workspace_dirty %}
本次fanout即將從HEAD開始建立worktree，偵測到源工作區仍有未提交程式碼，故提醒：
- 分叉源工作區：{{ fanout_workspace_path }}。請檢查是否有本次任務產生、且後續分支需要的業務改動尚未提交；如有，審閱後按具體路徑提交，可使用 Conventional Commits。
- 如無需要提交的改動，不做任何 Git 操作，直接重新輸出 artifact。不要求工作區乾淨或產生新 commit；之後不會再次因髒檔案阻止 fanout。
- 不要為了本提示清理工作區、stash 無關內容、移動其他 worktree、盲目 `git add -A` 或改變忽略規則。保留無關內容，不丟棄無法安全處理的改動。
- 重新輸出的 artifact 仍須滿足其他協定校驗，不要修改 schema 或添加 workspace/branch 欄位。
{% endif %}
不要輸出解釋、Markdown、程式碼圍欄或任何額外內容，只輸出修復後的 `dynamic-node-completion` 內容。

{% if has_coordination_snapshot %}最新協調快照：
- 唯讀快照：{{ coordination_snapshot_path }}
- 修復並輸出 `next.type="single"` 或 `next.type="fanout"` 前讀取最新協調快照；只能讀取，不要修改該檔案。
{% endif %}

校驗錯誤：
{{ validation_errors }}

目前合法值參考：
{{ repair_reference }}

目前剩餘預算：
{{ remaining_budget }}
