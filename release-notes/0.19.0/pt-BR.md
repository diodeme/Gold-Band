## Novos recursos e ajustes

### 1.Sessões Direct recebem mensagens em segundo plano e podem continuar ativas

Quando um Agent continua trabalhando depois de terminar uma resposta (por exemplo, ao disparar uma tarefa agendada ou executar um sub-Agent em segundo plano), a sessão Direct agora continua recebendo as respostas, chamadas de ferramentas, pedidos de permissão e perguntas dele, em vez de perdê-los quando a resposta termina.

1.Você é notificado uma vez quando chega uma nova resposta em segundo plano; a continuação da mesma resposta não gera nova notificação.
2.Enquanto o trabalho em segundo plano está em execução, o ícone da sessão na barra lateral pulsa e um indicador de carregamento substitui o horário; dentro da sessão você vê o que o Agent está pensando ou qual ferramenta está chamando.
3.Quando há atividade em segundo plano, o campo de mensagem mostra um botão de parar para interromper esse trabalho; o envio de novas mensagens não é afetado.
4.Os arquivos alterados por ferramentas em segundo plano entram nas alterações de arquivos do turno mais recente.

Clique com o botão direito em uma sessão Direct e escolha "Manter sessão ativa" para exibir um alfinete ao lado do título; clique com o botão direito novamente e escolha "Permitir liberação da sessão" para desfazer.

Todas as sessões Direct ativas são mantidas enquanto houver 8 ou menos. Acima de 8, as sessões ociosas há mais de 6 horas são liberadas; acima de 20, as sessões inativas há mais tempo são liberadas antecipadamente. Sessões mantidas ativas, em execução ou aguardando sua resposta a uma permissão ou pergunta nunca são liberadas.

> [!WARNING]
> Manter uma sessão ativa vale apenas enquanto o aplicativo está aberto; as sessões mantidas não são iniciadas automaticamente depois que o aplicativo reinicia. Tarefas agendadas em uma sessão que não está mantida ativa podem ser interrompidas quando ela é liberada. Parar o trabalho em segundo plano apenas envia um pedido de cancelamento ao Agent; cabe ao Agent decidir se remove as tarefas agendadas.

### 2.Diagramas Mermaid e alertas do GitHub no Markdown

Blocos de código ` ```mermaid ` em conversas e pré-visualizações de arquivos agora são renderizados como diagramas, com cores que seguem o tema atual. Clique em um diagrama para abri-lo no espaço de trabalho à direita, onde é possível ampliar, mover e copiar ou salvar como PNG.

Conversas e pré-visualizações de arquivos também aceitam os cinco alertas do GitHub: `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` e `[!CAUTION]`.

### 3.Imagens da web carregam apenas de domínios confiáveis

Para evitar que documentos ou respostas de Agents façam o cliente enviar requisições a servidores arbitrários, as imagens da web em Markdown agora só carregam automaticamente de domínios confiáveis.

1.Em conversas: imagens não confiáveis mostram um espaço reservado; clique em "Carregar" para confiar no domínio e exibir a imagem.
2.Em pré-visualizações de arquivos: imagens não confiáveis continuam como links, e um aviso no topo permite confiar em todos os domínios do documento de uma vez.
3.Veja e remova os domínios confiáveis em "Configurações → Avançado → Domínios de imagem confiáveis".

### 4.Ver resumos de compactação de contexto

Quando um Agent termina de compactar o contexto e retorna um resumo em texto, o registro da compactação mostra "Ver resumo". Clique para abrir o resumo no espaço de trabalho à direita. Ele é exibido como Markdown por padrão, e você pode alternar para o código-fonte e copiá-lo.

### 5.Obter configuração por modelo

Ao escolher um modelo ainda não usado em um nó de workflow, nas configurações do AUTO, no campo de mensagem da página inicial ou em uma sessão, aparecem "A configuração deste modelo ainda não foi obtida" e o botão "Obter configuração do modelo". Ao clicar, o Agent é iniciado uma vez com esse modelo para ler as opções reais; nenhuma mensagem é enviada e nenhuma tarefa é criada.

Você pode continuar editando, salvando e iniciando tarefas sem clicar. As configurações de modelo existentes não são mais cortadas com base na configuração de outro modelo, e uma falha no diagnóstico do Agent em segundo plano não apaga mais as configurações de modelo já obtidas.

### 6.Notas da versão no estilo do GitHub e disponíveis no menu Ajuda

As notas na caixa de diálogo de atualização agora são exibidas da mesma forma que no GitHub Releases, com quebras de linha e alertas. O novo "Ajuda → Notas da versão" permite ler as notas da versão atual a qualquer momento.

### 7.Novo layout do grafo de workflow

O editor de workflow e o grafo de execução usam um novo layout automático, com nós e conexões mais bem organizados. Os grupos criados dinamicamente pelo AI-DYNAMIC aparecem como quadros aninhados, e um indicador de carregamento é exibido quando o layout demora.


## Melhorias de experiência

### 1.Entregas do AUTO precisam passar pela validação

Quando o AUTO termina o desenvolvimento, o trabalho vai para um nó de validação que o confere com o requisito original. Se a validação falhar, o AUTO agenda correções e valida novamente; só termina depois que a validação é aprovada. A validação usa o requisito original como única base: um plano que exclui conteúdo do requisito é tratado como bloqueio, e testes existentes modificados precisam ser verificados novamente com suas versões originais.

Resumos longos e descrições de tarefas enviados pelos Agents agora são transferidos por arquivos, reduzindo novas tentativas causadas por erros de formato de saída. Quando o AUTO pausa, o aviso mostra o título específico do erro.

### 2.Atualização das integrações com Claude e Codex

O Claude ACP foi atualizado para `0.87.0` e o Codex ACP para `2.1.1`.

1.As sessões mostram o status da compactação de contexto, a duração e o uso antes da compactação.
2.A saída dos comandos executados pelo Codex continua se acumulando nos detalhes da ferramenta, até 256.000 caracteres.
3.Respostas personalizadas às perguntas do Claude podem ser enviadas corretamente.
4.Quando o Agent precisa que você entre novamente, um aviso claro é exibido; entre e depois reenvie manualmente.

### 3.Sessões mais estáveis

1.Ao voltar a uma sessão, o estado de carregamento não aparece mais repetidamente nem o conteúdo antigo é exibido primeiro.
2.Erros de execução, como disco cheio ou falta de permissão de gravação, aparecem imediatamente, e o campo de mensagem não fica mais aguardando; ferramentas sem resultado retornado mostram "Resultado não confirmado", e as respostas e rascunhos existentes são mantidos.
3.Depois que você para uma resposta, a sessão sempre encerra o turno atual corretamente, em vez de ficar em "Parando".

### 4.Registro de alterações de arquivos mais preciso

1.Corrigido o problema em que todas as alterações de um turno eram marcadas como incompletas quando Agents como o Claude editavam o mesmo arquivo várias vezes em fragmentos.
2.Corrigido o problema em que arquivos criados pelo Cursor eram registrados como modificações, com marcadores de Diff misturados ao conteúdo.
3.As linhas de alteração de arquivos mostram de forma consistente o nome do arquivo, o diretório e as linhas adicionadas/removidas; arquivos editados várias vezes também mostram o número de edições.

### 5.Outras melhorias

1.Pré-visualização de imagens unificada: imagens de arquivos e de sessões podem ser ampliadas diretamente com a roda do mouse, se ajustam à janela na primeira abertura e têm um botão "100%" para o tamanho original.
2.Quando o navegador integrado abre um HTML local, os recursos em diretórios superiores agora carregam corretamente; ao acessar um diretório fora do escopo permitido, você pode escolher "Permitir acesso" no aviso.
3.Os títulos nas conversas têm uma hierarquia mais clara e mantêm o espaçamento durante o streaming; "Notas da versão" foi movido para o final do menu Ajuda.
4.Corrigido o problema em que, com clientes de canais diferentes instalados, instalar ou atualizar um deles fechava o outro em execução.
5.ACP Registry e catálogo de Agents atualizados.
