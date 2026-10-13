## Melhorias de experiência

### 1.Correções no encerramento em segundo plano do Direct

1.Corrigido o botão Parar que não fazia nada quando uma pergunta em segundo plano aparecia depois que uma tarefa agendada acordava o Agent. Agora Parar age corretamente sobre o trabalho em segundo plano: um pedido de permissão pendente é registrado como cancelado, uma pergunta pendente recebe "cancelar" como resposta, e a sessão não fica mais presa esperando.
2.Pedidos de permissão e perguntas que chegam enquanto uma resposta está em execução agora são resolvidos com aquele turno, em vez de permanecerem na sessão depois que você para; interações já processadas não voltam mais ao estado pendente por causa de conteúdo ao vivo mais recente.

### 2.Atividade em segundo plano não fica mais presa

Alguns Agents não enviam a atualização final das ferramentas depois de parados, o que mantinha a sessão como ativa na barra lateral, deixava o botão de parar sempre visível e impedia que sessões mantidas ativas fossem liberadas. Agora as ferramentas são encerradas quando o turno responsável termina ou quando o trabalho em segundo plano é parado, e a sessão volta a ficar ociosa após 10 segundos sem atividade.

### 3.Correção do menu de modelo em painéis estreitos

Quando o painel da conversa é estreito, o submenu do modelo e do raciocínio era comprimido até a borda esquerda da janela e cobria a barra lateral. Sem espaço, ele agora se expande dentro do próprio menu principal (com clique ou teclas de seta); com espaço suficiente, continua abrindo ao lado ao passar o mouse, como antes.

### 4.Outras melhorias

1.Atualizados o snapshot do ACP Registry e o catálogo de Agents.
