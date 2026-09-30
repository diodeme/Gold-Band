## Novos recursos e ajustes

### 1.Gerenciamento de arquivos direto no espaço de trabalho

Agora você pode criar arquivos e pastas, renomear itens e excluí-los diretamente pela árvore de arquivos no espaço de trabalho à direita, sem alternar para o gerenciador de arquivos do sistema.

A criação e a renomeação são feitas diretamente na árvore. A exclusão pede confirmação e move o conteúdo para a Lixeira do sistema. Quando a árvore de arquivos estiver em foco, pressione `Ctrl+Z` (`Command+Z` no macOS) para desfazer a criação, renomeação ou exclusão mais recente.

![image.png](https://static.dion.blue/2026/09/20260930162214313.png)

### 2.Citação direta de arquivos e Diffs em uma conversa

O menu `@` do campo de mensagem agora permite navegar e pesquisar arquivos no espaço de trabalho atual. Ao selecionar um arquivo, uma referência é adicionada à mensagem para o Agent atual; os nós seguintes em Workflow e AUTO também recebem essas referências.

![image.png](https://static.dion.blue/2026/09/20260930163805780.png)

Selecione texto em um arquivo do espaço de trabalho, diretório de execução, anexo ou alteração de arquivo para citar as linhas correspondentes. Em um Diff, a seleção inclui automaticamente os blocos completos afetados, e o cabeçalho do arquivo permite citar o Diff inteiro. As citações preservam o caminho do arquivo, os números das linhas e a origem da alteração para que o Agent interprete o contexto com precisão.

![image.png](https://static.dion.blue/2026/09/20260930162523931.png)

### 3.Descarte de alterações de um arquivo no controle de código-fonte

As ações de arquivo no controle de código-fonte agora incluem “Descartar alterações”. Após a confirmação, as alterações preparadas e não preparadas são restauradas para `HEAD`; arquivos novos ainda não rastreados pelo Git são excluídos.

> [!attention] Alterações descartadas não podem ser recuperadas
> Esta ação não move o conteúdo para a Lixeira do sistema. Confirme que você não precisa mais das alterações locais do arquivo antes de continuar.

![image.png](https://static.dion.blue/2026/09/20260930162543785.png)

### 4.Novo acesso às atualizações e novo fluxo de instalação

Quando há uma nova versão, um botão persistente “Atualizar” aparece na barra de título. Nele, você pode ler as notas da versão, ampliar as imagens das notas e iniciar a atualização. O diálogo mostra o progresso do download, permite tentar novamente em caso de falha e solicita a reinicialização para instalar após a conclusão.

![image.png](https://static.dion.blue/2026/09/20260930162647062.png)

Agora a instalação salva as edições abertas, pausa as sessões e fecha conexões e serviços em segundo plano pelo fluxo normal de encerramento. Em seguida, o Gold Band verifica a assinatura e a versão da atualização baixada antes de instalar, reduzindo o risco de perder o estado do trabalho ou instalar o pacote errado.

Esta versão é uma atualização normal: o cliente apenas avisa sobre a nova versão e espera que você inicie manualmente o download e a instalação.

### 5.Reparo direcionado do cache do npx no gerenciamento de Agents

Quando o diagnóstico de um Agent npm/npx falha devido a um cache de instalação incompleto, o gerenciamento de Agents oferece “Reparar cache”. Depois que você confere os diretórios de destino, o Gold Band remove apenas os diretórios de instalação do npx envolvidos no erro e verifica o Agent novamente de forma automática.

O reparo não limpa todo o cache do npm nem remove arquivos do projeto, pacotes globais ou configurações da conta. Agents que estejam usando esses diretórios de cache podem ser interrompidos; encerre primeiro as tarefas relacionadas.

![image.png|500](https://static.dion.blue/2026/09/20260930162937093.png)
![image.png|500](https://static.dion.blue/2026/09/20260930162952757.png)

## Melhorias de experiência

### 1.O espaço de trabalho à direita acompanha o worktree da sessão

A navegação, edição e pesquisa de arquivos, os links de arquivos, o menu de arquivos `@` e o controle de código-fonte agora usam de forma consistente o worktree real da sessão atual. Ao mudar de nó ou worktree, o espaço de trabalho à direita também muda, sem ler por engano o diretório principal do projeto.

Se o worktree de uma sessão anterior já tiver sido removido, o Gold Band mostra claramente que ele está indisponível. A mudança para o espaço de trabalho principal só ocorre quando você escolhe “Navegar pelo espaço de trabalho principal”, sem fallback silencioso.

### 2.Otimização do controle de código-fonte para abertura mais rápida

O carregamento inicial, as atualizações em segundo plano e a revisão de commits ficaram mais responsivos, reduzindo a espera ao abrir o controle de código-fonte pela primeira vez. As informações do repositório e a lista de arquivos aparecem antes, e as estatísticas de linhas são preenchidas depois; a pesquisa de arquivos ignora os metadados de `.git`, e arquivos temporários de bloqueio do Git não provocam mais atualizações completas desnecessárias.

Também foram corrigidos atrasos na atualização do estado após mudanças nos arquivos e solicitações lentas que substituíam resultados mais recentes. Diffs abertos, termos de pesquisa e informações do repositório permanecem disponíveis durante a reconciliação em segundo plano.

### 3.Outras melhorias

1.O espaço de trabalho de arquivos agora visualiza SVG de forma nativa e pode abrir o arquivo atual com um aplicativo do sistema.
2.Os registros de alterações de arquivos em cada turno do Agent ficaram mais completos, inclusive para processamento em segundo plano ou alterações fora do worktree; evidências incompletas são identificadas com clareza.
3.Na inicialização, o aplicativo carrega somente o idioma ativo da interface, reduzindo leituras e trabalho desnecessários.
4.Corrigido o desaparecimento das bordas da janela após o redimensionamento nativo no Windows 10.
5.Corrigidos os tooltips de texto truncado que permaneciam abertos após ações de menu, os tooltips presos no canto superior esquerdo depois que seu acionador era ocultado e a perda de foco do campo de mensagem após a remoção de uma tag de função.
6.O canal padrão não exibe mais a entrada indisponível de Gerenciamento de requisitos, e vários erros de tradução restantes foram corrigidos.
