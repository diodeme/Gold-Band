## Auto-commit de desenvolvimento e testes

Após concluir a implementação do requisito e os testes automatizados obrigatórios, execute este passo de encerramento antes de finalizar o node.

1. Use `memory_read` para inspecionar `storyId` e `storyName` no escopo task. Reutilize ambos quando forem não vazios. Quando qualquer um estiver ausente, vazio ou não pareado, gere primeiro a identidade com `storyId=0` e as regras de extração do nome do requisito e tente `memory_write`. Para key ausente use `operation="create"` e omita `expectedRevision`; para key existente use `operation="update"` com sua revision. Se a ferramenta estiver indisponível ou a leitura falhar, informe que a identidade não foi lida; se a gravação ou verificação falhar, informe que não foi persistida. Nenhum caso é um bloqueador: pergunte ao usuário a identidade do requisito ou continue com a identidade confirmada nesta execução.
2. Identifique alterações relacionadas à task produzidas por este node, incluindo código, testes, prompts, documentos de design de produto e o plano de desenvolvimento. Exclua alterações que existiam antes da execução ou são claramente mudanças do usuário não relacionadas.
3. Se não houver alterações relacionadas à task, não crie commit vazio. Registre que nenhum commit foi necessário, a evidência e quaisquer alterações não commitadas retidas que não foram incluídas.
4. Selecione um token de tipo Conventional Commits padrão e uma descrição concisa em chinês a partir do diff real relacionado à task. Foque no resultado entregue, não no histórico de execução, nomes de modelos ou declarações genéricas.
5. Execute `git add` apenas com caminhos específicos. Nunca use `git add -A` e nunca inclua alterações não relacionadas. Crie um commit lógico após execução bem-sucedida do node.
6. A mensagem de commit deve conter exatamente três linhas:

```text
--story=[{storyId}] {storyName}
{type}: {中文描述}
#AI COMMIT#
```

7. Após o commit, verifique o OID do commit, a mensagem completa e o estado dos caminhos relacionados à task. Nunca declare conclusão do node quando o commit ou a verificação falhar.
8. Registre o OID do commit, a mensagem de três linhas, caminhos incluídos, caminhos excluídos e motivos no relatório de desenvolvimento e testes. O CI/CD usa esses OIDs de commit para determinar se esta task tem commits não enviados.
9. Não peça ao usuário para confirmar a mensagem ou o formato do commit. As regras existentes de permissão de comando ACP permanecem em vigor; nunca contorne limites de permissão para auto-commit.
10. Se o commit falhar, preserve o erro original e o estado atual do Git para o caminho existente de falha do node, recuperação manual ou falha de aceitação. Não crie commit substituto nem reescreva o histórico de commits.
