## Novidades e ajustes

### 1. Guia de primeira execução e descoberta de Agents locais

Instalações novas e usuários que ainda não concluíram a inicialização agora recebem um fluxo de configuração em três etapas:

1. Verifique o Git. Você pode ignorá-lo por enquanto e configurá-lo quando uma operação que dependa de Git for necessária.
2. Verifique Node.js, npm e npx. Os três precisam passar na verificação para continuar.
3. Descubra e diagnostique os Agents locais disponíveis. Agents diagnosticados com sucesso são adicionados automaticamente a “Gerenciamento de Agents”; escolha o Agent, o modelo e o modo de permissão padrão, revise as alterações e aplique-as a todos os Workflows integrados.

Quem já concluiu a inicialização pode executar o fluxo novamente em “Configurações → Geral → Orientação e Workflows”. A ação “Configurar padrões” atualiza em lote apenas os Workflows integrados e não sobrescreve Workflows personalizados.

> [!NOTE]
> O fluxo não instala Git, Node.js nem Agents automaticamente. Quando uma ferramenta está ausente, ele mostra as instruções oficiais de instalação; instale ou faça login e execute a detecção novamente. Um Agent só aparece como selecionável depois de passar pelo diagnóstico atual.

### 2. Rascunhos do Composer e fila de envio

Você pode continuar digitando enquanto uma sessão Direct está em execução e colocar mensagens em uma fila para enviá-las depois. A fila comporta até 10 itens e oferece envio automático ou manual, pausa e retomada, reordenação, edição, exclusão e envio imediato. Itens não enviados permanecem após parar a sessão ou reiniciar o cliente.

A nova caixa de rascunhos armazena até 20 rascunhos. Ela preserva texto, referências de arquivos, papel e referências de anexos até que você decida restaurá-los e enviá-los. Uma falha ao salvar não apaga o texto em edição, e os anexos não se perdem ao parar a sessão ou mudar de página.

### 3. Quadro de requisitos do Multica e entrega de resultados

A página “Requisitos” da edição interna agora oferece um fluxo mais completo para tarefas remotas:

1. Filtre por tarefa de desenvolvimento ou teste e pesquise pelo nome da tarefa. Tarefas de teste cujo desenvolvimento terminou exibem “Pronta” e ficam no início da fila; tarefas que exigem um requisito DPMS exibem a marca “DPMS”.
2. Nós designados do Workflow podem enviar snapshots intermediários da entrega, permitindo que tarefas posteriores obtenham as instruções antes do término da tarefa anterior; o nó final resume todo o item de trabalho. Depois da conclusão, você ainda pode complementar e atualizar as instruções na conversa original.
3. Quando as instruções de entrega da tarefa anterior ainda não estão prontas, um cartão na conversa permite continuar aguardando, informar os dados manualmente ou pular esta busca.
4. Tarefas concluídas ou com falha podem ser executadas novamente. A linha original volta para a fila e a nova tarefa recebe a marca “Executar novamente”.
5. Quando um papel ou Workflow é compartilhado pelo Multica Web, o cliente pode recebê-lo em segundo plano e atualizar a lista correspondente, sem uma nova importação no desktop.

### 4. Gerenciamento de papéis e papel WeAPM integrado

Em “Gerenciamento de contexto → Papéis”, agora é possível importar diretamente um ou vários arquivos Markdown, além de importar uma pasta em lote. Cada lote processa até 5.000 arquivos e mantém o motivo de sucesso ou falha de cada arquivo. Os papéis personalizados passam a aparecer em uma lista completa, sem paginação.

O canal WB ganhou um papel WeAPM integrado e somente leitura, que usa a CLI do WeAPM para consultar Trace e Log e investigar chamadas lentas, erros, timeouts e stacks de exceção.

> [!NOTE]
> O papel WeAPM está disponível apenas no canal WB e requer o serviço interno WeAPM, acesso à rede e a CLI `weapm`. As consultas são somente leitura.

### 5. Intervenção remota por IM e autorização do Feishu

As integrações de IM agora ficam em cartões independentes e recolhíveis, facilitando o gerenciamento separado do estado da conexão, autorização, vínculo e preferências de notificação. Na autorização do Feishu, você pode escolher um aplicativo existente ou criar um novo e trocar de aplicativo mais tarde. As respostas de intervenção remota permanecem consistentes após novas tentativas, reconexões e callbacks atrasados; um callback que não puder mais ser aplicado aparece como mensagem de acompanhamento.

> [!WARNING]
> **Trocar o aplicativo do Feishu remove o vínculo atual com o chat privado**
> Ao substituir o aplicativo, a autorização é trocada e o vínculo existente com o chat privado é removido. Depois de autorizar o novo aplicativo, envie outra mensagem privada ao bot usando a conta correspondente. Clientes antigos não conseguem ler diretamente os dados de IM atualizados; faça backup dos dados do aplicativo antes de atualizar se precisar manter a possibilidade de downgrade.

### 6. Canvas do Workflow e configurações de exibição do desktop

“Configurações → Aparência” agora inclui “Zoom da interface”, com escala do cliente inteiro de `50%–250%`. Também é possível usar `Ctrl+roda do mouse`, `Ctrl+mais/menos` e `Ctrl+0` para ajustar ou restaurar o padrão.

Os gráficos de autoria e execução do Workflow compartilham a preferência de layout “Automático / Horizontal / Vertical”. O gráfico de execução e o editor têm uma visualização em tela cheia que mantém o rascunho e o inspetor atuais; pressione `Esc` para sair.

## Melhorias de experiência

### 1. Encontrar e recuperar sessões com mais facilidade

A barra lateral agora inclui uma seção de “Sessões recentes” que reúne os espaços de trabalho por atividade. Clique em uma linha para abrir a sessão diretamente; se o resultado da busca ainda não estiver carregado, a linha correta é criada e revelada na barra lateral.

Ao iniciar uma sessão Direct, o cliente continua aguardando o ACP ficar pronto em vez de informar uma falha enquanto o Agent ainda inicia. Erros terminais confirmados continuam visíveis. Atualizações em segundo plano, paginação e troca de espaço de trabalho não deixam respostas atrasadas sobrescreverem sessões já exibidas.

### 2. Arquivos, anexos e estado de entrada mais confiáveis

Arquivos do espaço de trabalho, artefatos de execução, anexos de sessão e abas de arquivos do Git compartilham ações como copiar caminho, revelar no gerenciador de arquivos e referenciar na conversa, preservando a origem original de cada arquivo. Referências de anexos em rascunhos e filas também podem ser restauradas entre páginas. Ao abrir uma sessão pela busca, a barra lateral permanece expandida e focada no destino.

A entrada de textos longos, a rolagem de conversas por IM e as transições do espaço de trabalho à direita ficaram mais estáveis. Erros de inicialização do Direct, envio da fila, prévias de anexos e referências a arquivos locais ficam restritos à superfície correspondente, sem transformar toda a conversa ou o espaço de trabalho em erro.

### 3. Outras melhorias de estabilidade

1. Os gráficos de Workflow se ajustam ao canvas correto ao alternar a direção ou entrar e sair da tela cheia, reduzindo sobreposição entre arestas, nós e a barra lateral.
2. As listas de sessões recentes e tarefas usam paginação limitada e carregamento progressivo, em vez de ler todo o histórico de uma vez.
3. A recuperação de sessões Direct, a conclusão de intervenções pendentes e a sincronização de aprovações remotas ficaram mais consistentes, reduzindo solicitações reaparecendo e estados antigos de tarefas.

---

## 0.21.1: Pequenas melhorias desde a 0.21.0

### 1. Adiar a configuração inicial

Após a aprovação das verificações do ambiente, você pode escolher “Configurar depois” quando a detecção de Agents terminar ou falhar, entrar no aplicativo e concluir a configuração do Agent mais tarde em Configurações. É possível continuar mesmo sem um Agent disponível. Essa escolha e qualquer decisão anterior de ignorar o Git são preservadas após reiniciar.

### 2. Navegação da barra lateral e sessões recentes mais estáveis

Ajustamos o alinhamento e os estados de seleção da navegação fixa, dos espaços de trabalho, das sessões recentes e do histórico de execuções. As sessões recentes oferecem a mesma expansão e as mesmas ações de linha das listas de espaços de trabalho, e as atualizações preservam a seleção na seção atual. As listas carregam página por página pelo botão “Mostrar mais”; ao recolher e reabrir as listas de espaços de trabalho ou de sessões recentes, elas voltam ao primeiro lote de itens.

### 3. Documentação e downloads do site atualizados

O site e as páginas do Demo agora usam a marca MALING, com uma página de leitura da base de conhecimento e um menu de download por plataforma. A troca de documentos, o layout em telas estreitas e os pontos de entrada para download ficaram mais estáveis.
