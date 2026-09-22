Este turno de resposta terminou, mas o runtime ainda não recebeu o artifact deste node. O protocolo abaixo está sendo fornecido ou repetido; isso não significa que a task está concluída nem exige handoff antecipado. Decida se pretende finalizar este node.

- Se você não pretendia finalizar este node, continue executando diretamente dentro do escopo da task atual, workspace e permissões de ferramenta. Nenhuma tag de status é necessária, e não faça handoff antecipado apenas para responder a este prompt. Após continuar, produza o artifact abaixo quando considerar este node finalizado.
- Se considerar este node finalizado, produza o artifact abaixo com base no trabalho concluído. Não reaudite os objetivos da task ou requisitos de aceitação, nem adicione trabalho de negócio por causa deste prompt.
- Antes de emitir o artifact, se a task atual exigir relatório ou outro anexo e ainda não tiver sido escrito, grave-o no diretório attachments da tentativa atual; pule este passo se for desnecessário ou já estiver concluído.
{% if can_read_runtime_snapshot %}- Ao preparar o artifact, atualize um snapshot de runtime somente leitura apenas quando explicitamente exigido pelo contexto de runtime abaixo; leia apenas o caminho de snapshot declarado.
{% endif %}- Enquanto continua a execução, responda e use ferramentas normalmente. Apenas a saída final do artifact deve omitir explicações, Markdown e cercas de código.
{% if finalize_context %}
O contexto de runtime a seguir é apenas para esta normalização de resultado de controle:
{{ finalize_context }}
{% endif %}

Output artifact: {{ artifact }}
Output kind: {{ kind }}

Siga este protocolo apenas quando considerar este node finalizado e decidir emitir o artifact:
{{ schema }}{% if success_condition %}

o runtime avaliará subsequentemente o resultado do node usando esta condição:
{{ success_condition }}{% endif %}
