<div align="center">

<img src="../../web/public/logo.svg" alt="Gold Band" width="128" />

# Gold Band

> Aspiramos a ser el último cliente de escritorio para Agents que necesites
>
> La experiencia de los principales clientes de Agents más un sistema completo de workflows, para el desarrollo diario y el trabajo largo y desatendido en requisitos grandes

[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](../../LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#plataformas-e-idiomas)
[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)

[Descargar](https://github.com/diodeme/Gold-Band/releases) · [Vista previa de la interfaz en línea](https://gold-band.dion.blue/en/demo#)

<!-- README-I18N:START -->

[English](../../README.md) | [简体中文](./README.zh-CN.md) | [繁體中文](./README.zh-TW.md) | [日本語](./README.ja-JP.md) | [한국어](./README.ko-KR.md) | [Português (Brasil)](./README.pt-BR.md) | **Español**

<!-- README-I18N:END -->

</div>

---

![Gold Band](https://static.dion.blue/2026/09/index.png)

Gold Band es un cliente de escritorio de AI Agents para proyectos locales. Se conecta a los principales Agents, como Claude Code y Codex, mediante Agent Client Protocol (ACP): un único diseño de interacción y varios harnesses entre los que puedes cambiar. También ofrece workflows completos y orquestación AUTO, para que las tareas largas se mantengan estables y observables en lugar de depender de la suerte de una sola ejecución del modelo.

> [!TIP]
> ¿Quieres verlo antes de instalarlo? Abre la [vista previa de la interfaz en línea](https://gold-band.dion.blue/en/demo#), preferiblemente en un navegador de escritorio. La vista previa en línea es limitada; la experiencia de referencia es la del cliente de escritorio.

## Aspectos destacados

- **Un cliente para los principales Agents**: soporte integrado para Claude Code, Codex, Cursor, Gemini CLI, CodeBuddy, Goose, Qwen Code, OpenCode, Kimi Code, Amp y Pi, además de la integración personalizada de cualquier Agent compatible con ACP.
- **Tres modos de ejecución**: conversaciones DIRECT, WORKFLOW fijo y orquestación AUTO cubren desde preguntas rápidas hasta requisitos grandes.
- **Workflows con ingeniería**: configura el Agent, el modelo, el rol y la forma de evaluar el resultado de cada nodo; vuelve a una sesión anterior para repararla o inicia una nueva round para seguir implementando el requisito.
- **Modo AUTO para tareas grandes**: un nodo divide el objetivo en subtareas y las reparte; cada subtarea se ejecuta en su propio Git worktree, un nodo de merge combina los resultados, un nodo de accept los valida y la siguiente ronda se reparte según el resultado.
- **Lo que esperas de un cliente de Agents**: gestión de SKILL, MCP y roles (Profile), tareas programadas, visualización y edición de archivos, control de código fuente, navegador integrado, intervención remota y notificaciones por IM, además de fondos de pantalla, avatares, fuentes y temas.
- **Ligero**: construido con Tauri 2 y Rust. El instalador ocupa solo unas decenas de MB y el uso de memoria ronda los 300 MB con varias sesiones en paralelo.

## Agents compatibles

| Agents integrados | |
| --- | --- |
| Claude Code, Codex | Puntos de partida recomendados |
| Cursor, Gemini CLI, CodeBuddy, Goose, Qwen Code, OpenCode, Kimi Code, Amp, Pi | Integrados; la disponibilidad depende del entorno local y del soporte ACP de cada Agent |
| Agents personalizados | Cualquier Agent compatible con ACP se puede añadir manualmente en la Gestión de Agents |

## Modos de ejecución

### DIRECT

Parecido a usar el propio Agent. Gold Band no inyecta un system prompt de workflow; solo proporciona una interfaz de escritorio unificada, almacenamiento de sesiones, adjuntos, configuración de modelo y permisos, controles para detener y recuperar, y métricas de tokens y duración.

Adecuado para preguntas cotidianas, cambios de código, depuración y conversaciones de desarrollo que necesitan contexto persistente.

### WORKFLOW

Usa un workflow explícito. Cada nodo representa una ejecución de un Agent y puede tener su propio Agent, modelo, rol y forma de evaluar el resultado; las aristas definen las transiciones tras un éxito, un fallo o una confirmación manual. Después de una ejecución puedes volver a una sesión anterior para reparar problemas o iniciar una nueva round para seguir avanzando.

Adecuado para tareas que requieren etapas de desarrollo claras, revisión y pruebas independientes, ciclos de fallo y aceptación estructurada.

### AUTO / AI-DYNAMIC

AI-DYNAMIC propone los siguientes nodos a partir del objetivo: divide subtareas, las ejecuta en paralelo en worktrees separados, combina los resultados mediante un nodo de merge, los valida mediante un nodo de accept y reparte una nueva ronda según el resultado actual. El runtime de Gold Band valida las propuestas y controla el estado real de la ejecución; los Agents no pueden modificar el runtime directamente.

Adecuado para tareas grandes o complejas cuyo flujo completo no se puede definir de antemano, pero que aun así requieren límites de ejecución y observabilidad.

## Más capacidades

- **Conversaciones**: streaming, preguntas de seguimiento, recuperación del historial, reutilización de sesiones y sincronización opcional con sesiones externas; elige modelos, niveles de razonamiento, modos de permisos y Slash Commands desde el composer.
- **Adjuntos y artefactos**: selección de archivos, arrastrar y soltar, pegado de imágenes, referencias a archivos del workspace, vistas previas y archivado de artefactos de los nodos.
- **Observabilidad de la ejecución**: inspecciona mensajes del Agent, llamadas a herramientas, prompts de sistema, frames sin procesar, tokens, duración y estado de la ejecución.
- **Workspace**: exploración y edición de archivos en tiempo real, control de código fuente con Git y navegador integrado.
- **Automatización y colaboración**: tareas programadas, intervención remota y notificaciones por IM (por ahora, WeCom) y notificaciones del sistema.
- **Gestión de Agents y contexto**: gestiona Agents, Profiles, MCP, SKILL y contexto a nivel de usuario o de proyecto, con diagnóstico del entorno de los Agents.
- **Personalización**: temas, fondos de pantalla, fuentes, avatares personalizados de usuario y de Agent, y análisis personal de uso.

## Inicio rápido

1. Descarga un paquete de escritorio desde [Releases](https://github.com/diodeme/Gold-Band/releases) o compila desde el código fuente.
2. Abre Gold Band y añade un workspace local.
3. Activa Claude Code, Codex (por ahora, Claude Code o Codex ya debe poder iniciarse localmente en tu equipo) u otro Agent ACP en la Gestión de Agents y comprueba que el diagnóstico del entorno sea correcto.
4. Vuelve a la página de inicio de conversaciones y elige un modo de ejecución:
   - `DIRECT`: conversa de forma continua con un Agent seleccionado. Recomendado para el primer uso.
   - `WORKFLOW`: usa un workflow fijo para tareas con etapas claras y una validación más estricta.
   - `AUTO`: deja que AI-DYNAMIC divida y programe dinámicamente objetivos abiertos o complejos.
5. Escribe un requisito y revisa la salida, las solicitudes de interacción, los adjuntos, los artefactos y el estado de la ejecución en la vista de detalle de la conversación.

> [!IMPORTANT]
> El proyecto aún no dispone de una cuenta del Apple Developer Program, por lo que la versión para macOS no está firmada con Developer ID ni notarizada por Apple. Consulta la [macOS Installation and Troubleshooting Guide](../guide/macos-install.md) (en inglés) para ver las opciones de instalación y resolver problemas de Gatekeeper.

## Plataformas e idiomas

- **Plataformas**: hay paquetes para Windows, macOS y Linux. Windows 10 / 11 es la prioridad, seguido de los Mac con Apple Silicon e Intel; la versión para Linux todavía no se ha probado por completo.
- **Idiomas de la interfaz**: 简体中文, 繁體中文, English, 日本語, 한국어, Português (Brasil) y Español.

## Preguntas frecuentes

### ¿En qué se diferencia de los workflows internos de los Coding Agents?

Los workflows internos de los Coding Agents suelen ser «un Agent principal que orquesta sub-Agents» o «scripts que orquestan un Agent»; orquestan **sessions**. Gold Band orquesta **harnesses**. Cualquier Agent compatible con ACP puede ser un nodo: por ejemplo, Codex, con sus herramientas integradas de browser y computer use, como nodo de aceptación, y Pi, minimalista y rápido, como nodo de desarrollo. Los nodos pueden diferir en un harness completo, no solo en el contexto.

### ¿En qué se diferencia de clientes de Agents como Codex App?

Esos clientes se construyen en torno a un único Agent fijo. Gold Band se sitúa por encima de los Agents, por lo que puede cambiar entre Agents con arquitecturas distintas y combinarlos, aprovechando las capacidades de cada uno. La contrapartida es que Gold Band no puede intervenir dentro del loop de un Agent; funciones como reorientar a un Agent a mitad del loop con un prompt del usuario son más difíciles de construir.

### ¿En qué se diferencia de otros clientes ACP?

Gold Band nació de los workflows, y las capacidades de cliente ACP se construyeron encima. Sus workflows van más allá de una simple programación: criterios de aceptación, resúmenes del contexto previo, detener y reanudar, y la fusión de nodos en el modo AUTO. El runtime dirige el avance de los nodos y el manejo de fallos, de modo que cada Agent puede centrarse en su nodo actual.

## Estado y hoja de ruta

Problemas conocidos:

- WORKFLOW y AUTO se basan en validación adversarial y loops, así que consumen más tiempo y tokens que pedírselo directamente a un Agent, pero reducen el retrabajo.
- Aún no hay terminal integrado ni control remoto desde dispositivos móviles.

Hoja de ruta:

1. Seguir mejorando la experiencia del cliente y corrigiendo errores conocidos de la interfaz.
2. Hacer los workflows más robustos y fáciles de usar, por ejemplo, reejecutar cualquier nodo y crear workflows a partir de lenguaje natural.
3. Completar un requisito típico y muy complejo con WORKFLOW y AUTO para demostrar públicamente el valor de la orquestación.
4. Reestructurar en torno a una arquitectura client → p2p / relay → host, con soporte para directorios locales y remotos como workspaces y para varios clientes controlando un mismo host.

## Cuándo encaja

Gold Band encaja bien para:

- Usuarios que quieren un único cliente de escritorio para varios Coding Agents locales.
- Tareas de desarrollo que necesitan conversaciones continuas, recuperación del historial y colaboración con adjuntos.
- Trabajo de larga duración que separa desarrollo, revisión, pruebas y aceptación.
- Tareas que necesitan registros del proceso, artefactos y recuperación ante fallos.

Gold Band todavía no encaja para:

- Entornos de producción que requieren un SLA comercial estable.
- Cargas de trabajo que dependen de Agents ACP o funciones de Providers que aún no tienen soporte completo.

## Desarrollo local

```bash
npm install
npm run dev
```

Comandos de verificación habituales:

```bash
cargo check
npm run web:test
npm run web:build
```

## Stack tecnológico

- Rust
- React
- Tauri 2
- Tailwind CSS
- shadcn/ui
- prompt-kit
- Agent Client Protocol / ACP

## Comunidad y comentarios

Este proyecto participa activamente en la [comunidad linux.do](https://linux.do) y la apoya. Las estrellas, las pruebas, los issues y los pull requests sobre integración de Agents, experiencia de conversación, workflows, calidad de la descomposición en AUTO y recuperación de errores son bienvenidos.

AGPL-3.0-only. Consulta [LICENSE](../../LICENSE).
