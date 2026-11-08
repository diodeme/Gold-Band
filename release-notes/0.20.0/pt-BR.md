## Recursos novos e alterados

### 1.Intervenção remota pelo Feishu / Lark

Além do WeCom, agora você também pode tratar pelo Feishu / Lark as solicitações de permissão, perguntas de acompanhamento e verificações manuais que bloqueiam uma tarefa, e receber notificações de Run e de fim de turno.

1. Abra "Configurações → Geral → Intervenção remota e notificações de IM", expanda "Feishu / Lark" e clique em "Escanear para conectar".
2. Confira as permissões necessárias, clique em "Concordar e gerar código QR" e escaneie o código com o Feishu / Lark no celular. Você pode escolher um app do Feishu / Lark existente ou criar um novo.
3. Após a autorização, envie uma mensagem ao bot no Feishu / Lark para vincular o chat privado individual.
4. Quando uma tarefa precisar de você, o Feishu / Lark recebe um cartão interativo com o ID, o nó, a tarefa e o workspace. Nele você pode permitir ou negar permissões, responder perguntas de acompanhamento (escolha única, múltipla escolha, respostas personalizadas e texto) e marcar verificações manuais como aprovadas ou reprovadas.

Depois do envio, o cartão mantém suas respostas e desativa os controles. Para solicitações já tratadas no desktop ou no WeCom, o cartão mostra onde foram tratadas. Solicitações encerradas por uma parada, um tempo limite ou motivos semelhantes aparecem como "Cancelada", nunca como recusada. Se um envio demorar para ser processado e acabar não sendo aceito, o bot envia uma mensagem separada explicando o motivo.

Em "Menu de configuração" você pode vincular novamente a conta de recebimento, trocar o app do Feishu / Lark, autorizar novamente ou excluir a configuração.

> [!NOTE]
> Um bot se conecta a apenas um dispositivo. Por enquanto só há suporte a chats privados individuais, e um cartão do Feishu / Lark comporta no máximo 5 perguntas de acompanhamento; trate solicitações maiores no desktop. A permissão para ler seu nome é opcional; sem ela, a conta de recebimento aparece como "Chat privado vinculado".

> [!WARNING]
> Após a atualização, os dados de IM são migrados para um novo formato que clientes de versões anteriores não conseguem mais ler; para fazer downgrade, é preciso restaurar um backup antes. Cartões do WeCom enviados antes da atualização e ainda pendentes ficam inválidos; trate essas solicitações no desktop.

### 2.Enviar agora e Envio automático para mensagens pendentes

Agora você tem mais controle sobre quando as mensagens enfileiradas durante uma resposta do Agent são enviadas.

1. "Enviar agora" envia uma mensagem pendente imediatamente. Se houver uma resposta em andamento, você precisa confirmar antes se quer interrompê-la; depois de marcar "Não perguntar novamente", a confirmação pode ser reativada em "Configurações → Avançado → Mensagens pendentes".
2. Chave "Envio automático": ligada, as mensagens pendentes são enviadas uma a uma ao fim de cada resposta; desligada, elas ficam na fila até você enviar cada uma com "Enviar agora".
3. Quando a sessão é interrompida ou o envio anterior falha, a fila mostra que está pausada e o motivo; clique em "Retomar" para continuar o envio automático.
4. Limpe a fila de pendentes em "Mais ações".

### 3.Rascunhos

A barra de ferramentas do campo de entrada agora tem "Rascunhos". Clique em "Salvar nos rascunhos" para guardar o que você digitou e, depois, em "Mover para a entrada" para continuar editando. Os rascunhos ficam salvos apenas na sessão atual, nunca são enviados automaticamente, e cada sessão guarda até 20.

### 4.Sub-Agents nativos do Claude

Com o Claude ACP `0.90.0` ou posterior, o Gold Band lê os estados dos sub-Agents informados pelo Claude e mostra com precisão em execução, aguardando interação, concluído, interrompido e com falha. Sub-Agents não aparecem mais como interrompidos só porque o turno principal terminou.

1. Enquanto houver sub-Agents em execução, um resumo deles aparece acima do campo de entrada; clique em um para ver o registro completo no workspace à direita.
2. Depois que o turno principal termina, o campo de entrada mantém o botão "Parar" enquanto algum sub-Agent continuar em execução em segundo plano ou aguardando permissão.
3. Quando a sessão tem trabalho em segundo plano, a dica de uso de Token informa "Não inclui o uso de ativações em segundo plano".

> [!IMPORTANT]
> O Claude ACP foi atualizado para `0.90.0`. Se a sua versão for antiga demais, você será avisado para atualizar; o Gold Band não altera suas configurações de instalação automaticamente.

### 5.Confirmar o destino antes do push

Ao fazer push de um branch no controle de código-fonte, primeiro são exibidos o branch de origem, o repositório remoto de destino e o branch de destino, e o push só acontece após a confirmação. Para um branch publicado pela primeira vez, você pode escolher o repositório remoto e definir o branch de destino como upstream após a publicação. Branches sem upstream são marcados como `new`.

### 6.Abrir a busca de sessões com Ctrl+K

Pressione `Ctrl+K` (`Command+K` no macOS) para abrir a busca de sessões; pressionar de novo com ela aberta seleciona a palavra-chave, e `Esc` fecha. `Ctrl+F` fica reservado para a busca dentro de arquivos nos editores.


## Melhorias de experiência

### 1.Página de configurações e cartões de IM

1. "Intervenção remota e notificações de IM" agora mostra WeCom e Feishu / Lark como dois cartões recolhíveis, com os tipos de notificação agrupados em "Precisa da sua ação" e "Resultados de execução".
2. Os cartões do WeCom e do Feishu / Lark seguem o idioma do desktop e oferecem suporte aos sete idiomas da interface.
3. Cartões do WeCom já tratados mostram todas as perguntas e respostas personalizadas. Cartões do WeCom de escolha única com várias perguntas comportam no máximo 3 perguntas; trate solicitações maiores no desktop.

### 2.Sessões mais estáveis

1. Quando um Agent falha durante a inicialização, as novas tentativas automáticas voltam a ser executadas corretamente; turnos que falham depois que a mensagem já chegou ao Agent passam para recuperação manual, evitando execução duplicada. Durante a inicialização, o campo de entrada mostra "Preparando Agent".
2. Quando não é possível criar uma sessão, o motivo da falha é exibido diretamente, em vez de ficar carregando.
3. Perguntas e solicitações de permissão que sobraram após o cliente fechar de forma inesperada ficam inválidas; ao respondê-las, uma mensagem clara é exibida em vez de indicar sucesso.
4. Ao continuar uma sessão, o modelo e o modo de permissão originais dela são mantidos.
5. Corrigido o uso de Token contado em dobro ao continuar uma sessão Direct depois que ela foi recuperada.

### 3.Rolagem mais natural

1. A lista de mensagens continua presa ao final quando o campo de entrada fica mais alto.
2. Enquanto você lê o histórico, conteúdo novo não puxa a tela de volta para o final; rolar para cima no topo carrega o histórico mais antigo, mesmo quando o conteúdo ocupa menos de uma tela.
3. A posição de rolagem do campo de entrada não salta mais ao digitar rascunhos longos.

### 4.Outras melhorias

1. Os menus `/` e `@` do campo de entrada recarregam skills e papéis sempre que são abertos, então skills novas ficam disponíveis sem reiniciar.
2. Anexos com o mesmo nome não sobrescrevem mais anexos anteriores na conversa; quando o diretório de dados da sessão está aninhado fundo demais para salvar um anexo, uma mensagem clara é exibida.
3. Imagens locais nas conversas são resolvidas em relação ao workspace da sessão; o navegador integrado mantém parâmetros de consulta e âncoras ao abrir links HTML locais.
4. A pré-visualização ao vivo de Markdown ocupa toda a largura do painel; o tamanho de fonte padrão do editor passou a ser 13px.
5. Quando uma aba do workspace à direita falha, só essa aba mostra "Não foi possível exibir esta aba"; as demais não são afetadas.
6. Servidores MCP iniciados via `npx` ou `uvx` agora aguardam até 180 segundos pelo download das dependências na primeira verificação, em vez de falhar durante o download.
7. Quando o idioma do sistema é chinês, o idioma da interface é escolhido pela marcação de escrita simplificada ou tradicional, sem ser afetado pela região.
8. Corrigidas dicas de ferramenta que apareciam sozinhas após clicar em diálogos ou menus.
9. Atualizados o ACP Registry e o catálogo de Agents.
