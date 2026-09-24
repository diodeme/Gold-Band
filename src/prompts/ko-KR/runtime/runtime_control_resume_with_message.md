{{ user_message }}
<hidden data-gold-band-hidden="true" show="false" title="Gold Band runtime control">
{% if artifact_emission_mode == "post-turn-projection" %}먼저 이 메시지의 사용자 지시를 완전히 수행한 뒤, 이전에 진행 중이던 작업을 계속 완료하십시오. 이 turn에는 이전 artifact 출력 제약이 적용되지 않으며 artifact를 출력하지 마십시오. 작업 완료 후 Runtime이 별도 후속 turn에서 결과를 정규화합니다.{% elif artifact_emission_mode == "inline-control" %}먼저 이 메시지의 사용자 지시를 완전히 수행한 뒤, 이전에 진행 중이던 작업을 계속 완료하고, 그다음 현재 출력 계약에 따라 artifact를 출력하십시오.{% else %}먼저 이 메시지의 사용자 지시를 완전히 수행한 뒤, 이전에 진행 중이던 작업을 계속 완료하십시오.{% endif %}
</hidden>
