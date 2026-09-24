## Pré-verificação de identidade do requisito

Execute esta seção antes da enumeração de topologia, Interview Round 0 ou enumeração de ramos Grill. No início desta seção, chame primeiro `memory_read` e depois execute a verificação de identidade.

1. Use `memory_read` para ler a memória atual de task e workspace. Inspecione apenas as entradas `task` no snapshot retornado; um `storyId` ou `storyName` de workspace não conta como identidade de task atual existente.
2. Continue o Workflow do papel original apenas quando a task contiver `storyId` e `storyName` e ambos os valores forem não vazios após trim.
3. Se qualquer key estiver ausente, vazia ou não pareada, pergunte exatamente uma questão sobre a identidade do requisito. As escolhas semânticas obrigatórias são:
   - `不存在`
   - `其他（用户自行输入）`
4. Use `AskUserQuestion` ou ferramenta de elicitação equivalente quando disponível. Caso contrário, produza uma pergunta em texto simples, mas preserve as mesmas duas escolhas semânticas. Esta pergunta não é um passo de topologia, round de ambiguidade, round de Interview ou ramo Grill. Continue o Workflow original após a resposta do usuário.
5. Quando o usuário escolher `不存在`:
   - Defina `storyId` como a string `0`.
   - Extraia um `storyName` curto do requisito: prefira título explícito ou resumo da primeira linha, depois a frase nominal central no objetivo, e use `系统需求` apenas se a extração continuar impossível.
   - Remova rótulos de template, notas de status, marcadores Markdown e pontuação irrelevante. Salve uma linha com no máximo 40 caracteres Unicode.
6. Quando o usuário escolher `其他（用户自行输入）`, obtenha o ID e o nome do requisito. Se o texto livre não puder ser claramente dividido nos dois valores, pergunte mais uma vez; se continuar incerto, aguarde esclarecimento em vez de adivinhar. Faça trim de ambos os valores e armazene cada um como uma linha.
7. Use `memory_write` para gravar ambas as entradas no escopo task. Antes de gravar, use as revisions das keys alvo do `memory_read` mais recente: use `operation="create"` e omita `expectedRevision` quando a key estiver ausente; use `operation="update"` com a revision correspondente ao corrigir uma key existente. Nunca grave essas keys no escopo workspace.
8. Após as duas gravações, chame `memory_read` novamente e confirme que `storyId` e `storyName` no escopo task correspondem exatamente aos valores confirmados pelo usuário.
9. Se `gold-band-memory` estiver indisponível ou a leitura falhar, informe que a identidade do requisito não foi lida e pergunte normalmente conforme o passo 3. Se uma gravação ou verificação pós-gravação falhar, informe que a identidade do requisito não foi persistida. Nenhum caso é um bloqueador. Continue com o `storyId` e `storyName` confirmados ou recém-fornecidos pelo usuário; um node posterior pode perguntar novamente.
10. Nunca edite arquivos de memória diretamente. Após obter a identidade, continue com a inicialização de Interview ou a enumeração da árvore de decisão Grill; mantenha a cópia no escopo task quando a gravação pela ferramenta for bem-sucedida.
