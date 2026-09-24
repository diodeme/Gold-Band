# Contexto de runtime AI-DYNAMIC para esta invocação

## Node dinâmico atual
- Parent node: {{ outer_node_id }}
- Parent attempt: {{ outer_attempt_id }}
- Dynamic run: {{ dynamic_run_id }}
- Internal node: {{ node_id }}
- Title: {{ title }}
- Kind: {{ kind }}
- Group: {{ group_id }}
- Chain: {{ chain_id }}
- Depth: {{ depth }}

## Localização de runtime
- Dynamic root: {{ dynamic_root }}
- Internal node (relativo à raiz Dynamic): {{ node_dir }}
- Attempt atual (relativo ao node interno): {{ attempt_dir }}
- Attachments (relativo ao attempt atual): {{ attachments_dir }}
- Workspace ID: {{ workspace_id }}
- Workspace path: {{ workspace_path }}
- Workspace capability:
{{ workspace_capability }}

{% if has_new_round_trigger %}
## Feedback do gatilho `$new-round`
{{ new_round_trigger }}
- Esta é a saída do node que falhou e abriu o Round novo atual. Entenda seu motivo de falha e trabalho inacabado antes de planejar as tasks internas deste Round; não repita simplesmente o requisito original inalterado.
- A prévia do artifact pode estar truncada. Leia o artifact ou attachments explicitamente listados quando precisar de detalhes completos.
{% endif %}

{% if has_coordination_snapshot %}
## Snapshot de coordenação de runtime
- Snapshot somente leitura (relativo à raiz Dynamic): {{ coordination_snapshot_path }}
- O runtime deriva este arquivo do grafo dinâmico canônico e é seu único escritor. Não o modifique.
- Leia o snapshot mais recente antes de iniciar ou continuar esta task: use goal, status TODO, relação pai e steps de cada `workstreams[]` para entender outras subtarefas, depois use aninhamento e fase de `groups[]` para evitar trabalho duplicado ou conflitante.
- Leia o mesmo caminho novamente antes de produzir `next.type="single"` ou `next.type="fanout"`, e planeje sucessores a partir do estado mais recente.
{% endif %}

{% if has_direct_predecessors %}
## Predecessores Direct
{{ direct_predecessors }}
{% endif %}

{% if has_active_group %}
## Grupo ativo
{{ active_group }}
{% endif %}

{% if has_inherited_groups %}
## Contexto de grupo herdado
{{ inherited_groups }}
{% endif %}

{% if has_siblings %}
## Irmãos paralelos
{{ siblings }}
{% endif %}

{% if has_available_attachments %}
## Attachments disponíveis
- Apenas caminhos de attachment são listados; conteúdos de attachment não são lidos nem inlined. Forme o caminho completo de uma entrada regular juntando `Dynamic root` com seus níveis sucessivos da árvore de caminhos; uma entrada de topo `absolutePath=` já está completa e deve ser usada como está.
{% if has_predecessor_attachments %}
### Cadeia de predecessores (cadeia de handoff de task que criou o node atual; até {{ source_predecessor_limit }} nodes)
{{ predecessor_attachments }}
{% if has_predecessor_attachment_overflow %}
- As listagens de attachment dos nodes de origem abaixo estão truncadas ou incompletas. No máximo {{ attachments_per_source_limit }} arquivos ou diretórios vazios são inspecionados por node; diretórios não vazios são percorridos e não consomem slot. Apenas arquivos encontrados são listados acima; inspecione os diretórios attachments completos conforme necessário:
{{ predecessor_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_dependency_attachments %}
### Dependências explícitas (nodes de entrada nomeados explicitamente via dependsOn pelo node atual)
{{ dependency_attachments }}
{% if has_dependency_attachment_overflow %}
- As listagens de attachment dos nodes de origem abaixo estão truncadas ou incompletas. No máximo {{ attachments_per_source_limit }} arquivos ou diretórios vazios são inspecionados por node; diretórios não vazios são percorridos e não consomem slot. Apenas arquivos encontrados são listados acima; inspecione os diretórios attachments completos conforme necessário:
{{ dependency_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_group_evidence_attachments %}
### Evidência de grupo (entradas atuais de merge / acceptance ou o merge e acceptance mais recentes de grupos relacionados)
{{ group_evidence_attachments }}
{% if has_group_evidence_attachment_overflow %}
- As listagens de attachment dos nodes de origem abaixo estão truncadas ou incompletas. No máximo {{ attachments_per_source_limit }} arquivos ou diretórios vazios são inspecionados por node; diretórios não vazios são percorridos e não consomem slot. Apenas arquivos encontrados são listados acima; inspecione os diretórios attachments completos conforme necessário:
{{ group_evidence_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% endif %}

{% if has_output_contract %}
## Reuso de sessão
- Session mode: {{ session_mode }}
- continueFromNodeId: {{ continue_from_node_id }}
- Nota: `continue` reutiliza apenas o contexto de sessão ACP do node de origem; a task atual é a `# Task` neste user prompt.
- Nodes de sessão retomáveis na cadeia atual:
{{ resumable_sessions }}

## Limites de runtime
- Snapshots de Workflow permitidos:
{{ allowed_workflow_snapshots }}
- Orçamento restante:
{{ remaining_budget }}

## Opções de Agent e profile
- Estratégia de Agent do node dinâmico: {{ agent_strategy_mode }}
- Bootstrap agent: {{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Orientação de roteamento de Agent:
{{ agent_routing_prompt }}
- Política de modelo merge / acceptance:
{{ acceptance_model_policy }}
{% endif %}- Agents disponíveis e opções de runtime configuradas:
{{ available_providers }}
- Profiles disponíveis:
{{ available_profiles }}
{% endif %}
