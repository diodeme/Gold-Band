{{ user_message }}
<hidden data-gold-band-hidden="true" show="false" title="Gold Band runtime control">
{% if artifact_emission_mode == "post-turn-projection" %}First, fully carry out the user instruction in this message. Previous artifact-output constraints do not apply in this turn, and do not output an artifact; the Runtime will normalize the result in a separate subsequent turn.{% elif artifact_emission_mode == "inline-control" %}First, fully carry out the user instruction in this message, then output the artifact according to the current output contract.{% else %}First, fully carry out the user instruction in this message.{% endif %}
</hidden>
