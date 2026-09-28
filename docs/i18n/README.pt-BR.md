<div align="center">

<img src="../../web/public/logo.svg" alt="Gold Band" width="128" />

# Gold Band

> Queremos ser o último cliente desktop de Agents de que você precisa
>
> A experiência dos principais clientes de Agents com um sistema completo de workflows, para o desenvolvimento do dia a dia e para trabalhos longos e autônomos em requisitos grandes

[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](../../LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#plataformas-e-idiomas)
[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)

[Download](https://github.com/diodeme/Gold-Band/releases) · [Prévia da interface online](https://gold-band.dion.blue/en/demo#)

<!-- README-I18N:START -->

[English](../../README.md) | [简体中文](./README.zh-CN.md) | [繁體中文](./README.zh-TW.md) | [日本語](./README.ja-JP.md) | [한국어](./README.ko-KR.md) | **Português (Brasil)** | [Español](./README.es.md)

<!-- README-I18N:END -->

</div>

---

Gold Band é um cliente desktop de AI Agents para projetos locais. Ele se conecta aos principais Agents, como Claude Code e Codex, por meio do Agent Client Protocol (ACP): um único design de interação e vários harnesses que você pode alternar. Também oferece workflows completos e orquestração AUTO, para que tarefas longas permaneçam estáveis e observáveis em vez de depender da sorte de uma única execução do modelo.

> [!TIP]
> Quer ver como é antes de instalar? Abra a [prévia da interface online](https://gold-band.dion.blue/en/demo#), de preferência em um navegador desktop. A prévia online é limitada; a experiência de referência é a do cliente desktop.

## Destaques

- **Um cliente para os principais Agents**: suporte integrado a Claude Code, Codex, Cursor, Gemini CLI, CodeBuddy, Goose, Qwen Code, OpenCode, Kimi Code, Amp e Pi, além da integração personalizada de qualquer Agent compatível com ACP.
- **Três modos de execução**: conversas DIRECT, WORKFLOW fixo e orquestração AUTO cobrem desde perguntas rápidas até requisitos grandes.
- **Workflows com engenharia**: configure Agent, modelo, papel e forma de avaliação do resultado para cada nó; volte a uma sessão anterior para corrigi-la ou inicie uma nova round para continuar implementando o requisito.
- **Modo AUTO para tarefas grandes**: um nó divide o objetivo em subtarefas e as distribui; cada subtarefa roda em seu próprio Git worktree, um nó de merge combina os resultados, um nó de accept os valida e a próxima rodada é distribuída com base no resultado.
- **O que você espera de um cliente de Agents**: gerenciamento de SKILL, MCP e papéis (Profile), tarefas agendadas, visualização e edição de arquivos, controle de código-fonte, navegador integrado, intervenção remota e notificações via IM, além de papéis de parede, avatares, fontes e temas.
- **Leve**: construído com Tauri 2 e Rust. O instalador tem apenas algumas dezenas de MB, e o uso de memória fica em torno de 300 MB com várias sessões em paralelo.

## Agents suportados

| Agents integrados | |
| --- | --- |
| Claude Code, Codex | Pontos de partida recomendados |
| Cursor, Gemini CLI, CodeBuddy, Goose, Qwen Code, OpenCode, Kimi Code, Amp, Pi | Integrados; a disponibilidade depende do ambiente local e do suporte ACP de cada Agent |
| Agents personalizados | Qualquer Agent compatível com ACP pode ser adicionado manualmente no Gerenciamento de Agents |

## Modos de execução

### DIRECT

Próximo de usar o próprio Agent. Gold Band não injeta um system prompt de workflow; apenas fornece uma interface desktop unificada, armazenamento de sessões, anexos, configuração de modelo e permissões, controles de parada e recuperação, além de métricas de tokens e duração.

Adequado para perguntas do dia a dia, alterações de código, depuração e conversas de desenvolvimento que precisam de contexto persistente.

### WORKFLOW

Usa um workflow explícito. Cada nó representa uma execução de Agent e pode ter seu próprio Agent, modelo, papel e forma de avaliação do resultado; as arestas definem as transições após sucesso, falha ou confirmação manual. Após uma execução, você pode voltar a uma sessão anterior para corrigir problemas ou iniciar uma nova round para seguir adiante.

Adequado para tarefas que precisam de etapas claras de desenvolvimento, revisão e testes independentes, ciclos de falha e aceitação estruturada.

### AUTO / AI-DYNAMIC

O AI-DYNAMIC propõe os próximos nós a partir do objetivo: divide subtarefas, executa-as em paralelo em worktrees separados, combina os resultados por um nó de merge, valida-os por um nó de accept e distribui uma nova rodada com base no resultado atual. O runtime do Gold Band valida as propostas e controla o estado real da execução; os Agents não podem alterar o runtime diretamente.

Adequado para tarefas grandes ou complexas cujo fluxo completo não pode ser definido antecipadamente, mas que ainda exigem limites de execução e observabilidade.

## Mais recursos

- **Conversas**: streaming, perguntas de acompanhamento, recuperação de histórico, reutilização de sessões e sincronização opcional com sessões externas; escolha modelos, níveis de raciocínio, modos de permissão e Slash Commands no composer.
- **Anexos e artefatos**: seleção de arquivos, arrastar e soltar, colagem de imagens, referências a arquivos do workspace, pré-visualizações e arquivamento de artefatos dos nós.
- **Observabilidade da execução**: inspecione mensagens do Agent, chamadas de ferramentas, prompts de sistema, frames brutos, tokens, duração e estado da execução.
- **Workspace**: navegação e edição de arquivos em tempo real, controle de código-fonte com Git e navegador integrado.
- **Automação e colaboração**: tarefas agendadas, intervenção remota e notificações via IM (por enquanto, WeCom) e notificações do sistema.
- **Gerenciamento de Agents e contexto**: gerencie Agents, Profiles, MCP, SKILL e contexto em nível de usuário ou de projeto, com diagnóstico do ambiente dos Agents.
- **Personalização**: temas, papéis de parede, fontes, avatares personalizados de usuário e de Agent, e análise pessoal de uso.

## Início rápido

1. Baixe um pacote desktop em [Releases](https://github.com/diodeme/Gold-Band/releases) ou compile a partir do código-fonte.
2. Abra o Gold Band e adicione um workspace local.
3. Ative Claude Code, Codex (por enquanto, o Claude Code ou o Codex já precisa iniciar localmente na sua máquina) ou outro Agent ACP no Gerenciamento de Agents e confirme que o diagnóstico do ambiente passou.
4. Volte à página inicial de conversas e escolha um modo de execução:
   - `DIRECT`: converse continuamente com um Agent selecionado. Recomendado para o primeiro uso.
   - `WORKFLOW`: use um workflow fixo para tarefas com etapas claras e validação mais rigorosa.
   - `AUTO`: deixe o AI-DYNAMIC dividir e agendar dinamicamente objetivos abertos ou complexos.
5. Digite um requisito e acompanhe a saída, solicitações de interação, anexos, artefatos e estado da execução na tela de detalhes da conversa.

> [!IMPORTANT]
> O projeto ainda não possui uma conta do Apple Developer Program, portanto a versão para macOS não é assinada com Developer ID nem notarizada pela Apple. Consulte o [macOS Installation and Troubleshooting Guide](../guide/macos-install.md) (em inglês) para opções de instalação e solução de problemas do Gatekeeper.

## Plataformas e idiomas

- **Plataformas**: há pacotes para Windows, macOS e Linux. O Windows 10 / 11 é a prioridade, seguido por Macs com Apple Silicon e Intel; a versão para Linux ainda não foi totalmente testada.
- **Idiomas da interface**: 简体中文, 繁體中文, English, 日本語, 한국어, Português (Brasil) e Español.

## Perguntas frequentes

### Qual a diferença em relação aos workflows internos dos Coding Agents?

Os workflows internos dos Coding Agents costumam ser "um Agent principal orquestrando sub-Agents" ou "scripts orquestrando um Agent"; eles orquestram **sessions**. O Gold Band orquestra **harnesses**. Qualquer Agent compatível com ACP pode ser um nó: por exemplo, o Codex, com suas ferramentas integradas de browser e computer use, como nó de aceitação, e o Pi, minimalista e rápido, como nó de desenvolvimento. Os nós podem diferir por um harness inteiro, e não apenas pelo contexto.

### Qual a diferença em relação a clientes de Agents como o Codex App?

Esses clientes são construídos em torno de um único Agent fixo. O Gold Band fica acima dos Agents, podendo alternar e combinar Agents com arquiteturas diferentes e aproveitar os recursos de cada um. A contrapartida é que o Gold Band não consegue entrar no loop interno de um Agent; recursos como redirecionar um Agent no meio do loop com um prompt do usuário são mais difíceis de construir.

### Qual a diferença em relação a outros clientes ACP?

O Gold Band nasceu dos workflows, e os recursos de cliente ACP foram construídos sobre eles. Os workflows vão além de um simples agendamento: critérios de aceitação, resumos do contexto anterior, parar e retomar, e a fusão de nós no modo AUTO. O runtime conduz a progressão dos nós e o tratamento de falhas, para que cada Agent possa se concentrar no nó atual.

## Status e roadmap

Problemas conhecidos:

- WORKFLOW e AUTO se baseiam em validação adversarial e loops, por isso consomem mais tempo e tokens do que pedir diretamente a um Agent, mas reduzem o retrabalho.
- Um terminal integrado e o controle remoto por dispositivos móveis ainda não estão disponíveis.

Roadmap:

1. Continuar melhorando a experiência do cliente e corrigindo bugs conhecidos de interface.
2. Tornar os workflows mais robustos e fáceis de usar, como reexecutar qualquer nó e criar workflows a partir de linguagem natural.
3. Implementar um requisito típico e altamente complexo com WORKFLOW e AUTO para demonstrar publicamente o valor da orquestração.
4. Reestruturar em uma arquitetura client → p2p / relay → host, com suporte a diretórios locais e remotos como workspaces e a vários clientes controlando um mesmo host.

## Quando usar

O Gold Band é uma boa opção para:

- Quem quer um único cliente desktop para vários Coding Agents locais.
- Tarefas de desenvolvimento que precisam de conversas contínuas, recuperação de histórico e colaboração com anexos.
- Trabalhos longos que separam desenvolvimento, revisão, testes e aceitação.
- Tarefas que precisam de registros do processo, artefatos e recuperação de falhas.

O Gold Band ainda não é indicado para:

- Ambientes de produção que exigem um SLA comercial estável.
- Cargas de trabalho que dependem de Agents ACP ou recursos de Providers ainda não totalmente suportados.

## Desenvolvimento local

```bash
npm install
npm run dev
```

Comandos de verificação comuns:

```bash
cargo check
npm run web:test
npm run web:build
```

## Stack técnica

- Rust
- React
- Tauri 2
- Tailwind CSS
- shadcn/ui
- prompt-kit
- Agent Client Protocol / ACP

## Comunidade e feedback

Este projeto participa ativamente e apoia a [comunidade linux.do](https://linux.do). Stars, testes, issues e pull requests sobre integração de Agents, experiência de conversa, workflows, qualidade da decomposição no AUTO e recuperação de erros são muito bem-vindos.

AGPL-3.0-only. Veja [LICENSE](../../LICENSE).
