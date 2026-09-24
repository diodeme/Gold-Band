A última proposta `dynamic-node-completion` não foi aceita. Trate os itens de validação ou o lembrete abaixo e reenvie-a.

Você deve reparar a saída final `dynamic-node-completion` para que satisfaça as restrições de runtime abaixo.
Repare apenas erros de validação de protocolo; não reexecute a task. O trabalho sucessor ainda deve satisfazer o contrato de escopo; remova ou estreite itens fora do escopo.
{% if fanout_workspace_dirty %}
Este fanout está prestes a criar worktrees a partir de HEAD. Código não commitado foi detectado no workspace de origem, então observe:
- Fork source workspace: {{ fanout_workspace_path }}. Verifique se esta task tem alterações de negócio não commitadas necessárias para ramos sucessores. Se sim, revise e faça commit desses caminhos específicos, opcionalmente usando Conventional Commits.
- Se nada precisar de commit, não execute operações Git e reenvie o artifact diretamente. Nem workspace limpo nem novo commit são exigidos; arquivos sujos restantes não bloquearão fanout novamente.
- Não limpe o workspace, não faça stash de conteúdo não relacionado, não mova outros worktrees, não use `git add -A` cegamente nem altere regras de ignore por causa deste lembrete. Preserve conteúdo não relacionado e quaisquer alterações que não possam ser tratadas com segurança.
- O artifact reenviado ainda deve passar em toda validação de protocolo restante. Não altere o schema nem adicione campos workspace/branch.
{% endif %}
Não produza explicações, Markdown, cercas de código ou qualquer texto extra. Produza apenas o conteúdo reparado de `dynamic-node-completion`.

{% if has_coordination_snapshot %}Snapshot de coordenação mais recente:
- Snapshot somente leitura: {{ coordination_snapshot_path }}
- Leia o snapshot de coordenação mais recente antes de reparar e produzir `next.type="single"` ou `next.type="fanout"`; leia apenas e não modifique este arquivo.
{% endif %}

Erros de validação:
{{ validation_errors }}

Referência de valor válido atual:
{{ repair_reference }}

Orçamento restante atual:
{{ remaining_budget }}
