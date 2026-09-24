Seu passo final deve produzir apenas o conteúdo JSON do artifact `dynamic-node-completion`. Não produza explicações, Markdown, cercas de código ou qualquer texto extra.

{% if agent_strategy_mode == "fixed" %}
Este node AI-DYNAMIC usa a estratégia fixed-agent: exceto para `workflow-invocation`, todos os nodes internos worker, merge e acceptance usarão o mesmo provider fixo escolhido pelo runtime. Não produza campos provider para nenhum node.
{{ model_policy }}
{% else %}
Este node AI-DYNAMIC usa a estratégia dynamic-agent: escolha e produza provider apenas para workers posteriores com base na orientação de roteamento e providers disponíveis neste prompt. Merge / acceptance sempre usam o bootstrap Agent, então não produza provider para eles. Não produza `model` ou `permissionMode` para nenhum node; o runtime lê a configuração salva.
{{ model_policy }}
{% endif %}

O JSON Schema abaixo é o protocolo de saída efetivo desta execução. O runtime o gerou a partir das estruturas de dados Rust e o restringiu com a configuração AI-DYNAMIC atual. Sua saída deve satisfazê-lo; o runtime usa o mesmo schema para validação e diagnósticos de reparo.

```json
{{ json_schema }}
```

Lembretes de restrição:
- Uma task sucessora só pode decompor um resultado estabelecido no escopo ou reparar um `BLOCKER` qualificado. Não promova um `FOLLOW_UP` ou sugestão de predecessor em um novo resultado. Deriva de escopo só pode agendar restauração da solução mínima no escopo.
{% if agent_strategy_mode == "fixed" %}- Sob a estratégia fixed-agent, não produza campos `provider`. O runtime injeta o Agent fixo automaticamente.
{% else %}- Sob a estratégia dynamic-agent, workers devem produzir um provider válido que siga a orientação de roteamento neste prompt; `merge / acceptance` devem omitir provider porque o runtime sempre usa o bootstrap Agent.
- Não produza `provider` para `workflow-invocation`.
{% endif %}- {{ model_policy }}
- Quando `next.type="end"`, não inclua `node / groupId / nodes / merge / acceptance`.
{% if end_summary_is_outer_handoff %}- Se usar `next.type="end"`, `summary` deve ser um handoff de negócio completo para o sucessor fora do AI-DYNAMIC: declare o que foi concluído, conclusões-chave, saídas importantes e preocupações restantes. Não descreva apenas roteamento nem diga "accepted".
{% else %}- Se usar `next.type="end"`, `summary` é um relatório interno de progresso ou de ramo. Declare com precisão o que este node concluiu para o manifesto de relatório do Runtime e o grupo envolvente.
{% endif %}
- Quando `next.type="single"`, você deve fornecer um `next.node` completo e não deve fornecer `groupId / nodes / merge / acceptance`.
- Não produza `workspace`, modo de workspace, caminho ou ramo para nenhum node. O runtime detém exclusivamente a atribuição de workspace.
- Um sucessor `next.type="single"` herda automaticamente o workspace real do node atual.
- Se este node for uma acceptance de grupo, aceitar sua saída válida fecha esse grupo: `single` retoma o ramo de negócio original no escopo pai; `fanout` cria um novo grupo no escopo pai; apenas `end` encerra esse ramo. Com sucessores, o grupo pai continua aguardando. Organize explicitamente reparos e verificação; o grupo antigo nunca reabre automaticamente.
- Quando `next.type="fanout"`, você deve fornecer `groupId / nodes / merge / acceptance` juntos, e `nodes` deve conter pelo menos dois ramos; use `next.type="single"` para um node sucessor.
- Todo filho `next.type="fanout"` recebe automaticamente um worktree isolado; merge e acceptance retornam automaticamente ao workspace pai desse grupo.
- Worktrees filhos fan-out herdam uma revision commitada, nunca conteúdo não commitado; o Runtime não faz checkpoint automaticamente. Se esta task tiver alterações de negócio não commitadas necessárias para ramos sucessores, revise e faça commit desses caminhos específicos, opcionalmente usando Conventional Commits. Se nada precisar de commit, não execute operações Git.
- Workspace limpo não é um gate de fanout. O runtime dá um lembrete na primeira detecção de workspace sujo; depois reenvie o artifact, sem exigir novo commit. Não limpe o workspace, não faça stash de conteúdo não relacionado, não mova outros worktrees, não use `git add -A` cegamente nem altere regras de ignore por causa deste lembrete. Deixe conteúdo não relacionado inalterado.
- `profile` é permitido apenas em nodes worker e é opcional. Se presente, use um ID do enum do schema ou o ID após `profileId=...` neste prompt, não o displayName.
- Não produza `profile` para `merge` / `acceptance`; o runtime usa os prompts AI-DYNAMIC integrados de merge / acceptance.
{% if agent_strategy_mode == "dynamic" %}- Se `provider` estiver presente, deve ser um dos providers disponíveis listados no enum do schema ou neste prompt.
{% endif %}- Se `sessionMode` for omitido, é tratado como `new`; use `continue` apenas ao retomar um node de sessão reutilizável na cadeia atual.
- Quando `sessionMode="continue"`, você deve fornecer `continueFromNodeId`, e ele deve referenciar um dos nodes de sessão retomáveis listados neste prompt.
- Não use `sessionMode="continue"` para `workflow-invocation`.
- Se `workflowId` estiver presente, deve ser um dos IDs de Workflow DSL permitidos listados no enum do schema ou neste prompt.
- A contagem de nodes fan-out deve satisfazer `minItems/maxItems` do schema, `maxFanout` e restrições de orçamento restante mostradas neste prompt.
- Produza apenas o JSON final. Não produza pseudocódigo, comentário ou exemplos encapsulados.
