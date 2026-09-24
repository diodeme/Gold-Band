{{ user_message }}
<hidden data-gold-band-hidden="true" show="false" title="Gold Band runtime control">
{% if artifact_emission_mode == "post-turn-projection" %}請先完整執行本訊息中的使用者指令，然後繼續完成你之前的任務。本 turn 不適用此前的 artifact 輸出約束，也不要輸出 artifact；完成後再由 Runtime 在後續獨立 turn 中完成結果歸一化。{% elif artifact_emission_mode == "inline-control" %}請先完整執行本訊息中的使用者指令，然後繼續完成你之前的任務，完成後再按目前輸出契約輸出 artifact。{% else %}請先完整執行本訊息中的使用者指令，然後繼續完成你之前的任務。{% endif %}
</hidden>
