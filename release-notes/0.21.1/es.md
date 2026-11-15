## Novedades y ajustes

### 1. Guía de primer inicio y detección de Agents locales

Las instalaciones nuevas y los usuarios que aún no han completado la inicialización reciben ahora un flujo de configuración de tres pasos:

1. Comprobar Git. Puedes omitirlo por ahora y configurarlo cuando necesites una operación que dependa de Git.
2. Comprobar Node.js, npm y npx. Los tres deben superar la comprobación para continuar.
3. Detectar y diagnosticar los Agents locales disponibles. Los Agents diagnosticados correctamente se añaden automáticamente a «Administración de Agents»; después puedes elegir Agent, modelo y modo de permisos predeterminados, revisar los cambios y aplicarlos a todos los Workflows integrados.

Los usuarios que ya completaron la inicialización pueden volver a ejecutar el flujo desde «Configuración → General → Guía y Workflows». La acción «Configurar valores predeterminados» actualiza en lote solo los Workflows integrados y no sobrescribe los personalizados.

> [!NOTE]
> La guía no instala Git, Node.js ni Agents automáticamente. Si falta una herramienta, muestra la guía oficial de instalación; instálala o inicia sesión y vuelve a ejecutar la detección. Un Agent solo aparece como seleccionable después de superar el diagnóstico actual.

### 2. Borradores del Composer y cola de envío

Puedes seguir escribiendo mientras se ejecuta una sesión Direct y poner los mensajes en una cola para enviarlos después. La cola admite hasta 10 elementos y ofrece envío automático o manual, pausa y reanudación, cambio de orden, edición, eliminación y envío inmediato. Los elementos no enviados se conservan al detener la sesión o reiniciar el cliente.

La nueva bandeja de borradores permite guardar hasta 20 borradores. Conserva el texto, las referencias de archivos, el rol y las referencias de adjuntos hasta que decidas restaurarlos y enviarlos. Si falla el guardado, el texto en edición no se borra y los adjuntos no se pierden al detener la sesión o cambiar de página.

### 3. Panel de requisitos de Multica y entrega de resultados

La página «Requisitos» de la edición interna ofrece ahora un flujo más completo para las tareas remotas:

1. Filtra por tarea de desarrollo o de prueba y busca por nombre. Las tareas de prueba cuyo desarrollo ha terminado muestran «Lista» y pasan al principio de la cola; las tareas que requieren un requisito DPMS muestran la marca «DPMS».
2. Los nodos designados del Workflow pueden enviar instantáneas intermedias de la entrega para que las tareas posteriores obtengan instrucciones antes de que termine la tarea anterior; el nodo final resume todo el elemento de trabajo. Después de completar la tarea, todavía puedes ampliar y actualizar las instrucciones desde la conversación original.
3. Si las instrucciones de entrega de la tarea anterior aún no están disponibles, una tarjeta dentro de la conversación permite seguir esperando, proporcionar la información manualmente u omitir esta obtención.
4. Las tareas completadas o fallidas se pueden «Volver a ejecutar». La fila original vuelve a la cola y la nueva tarea recibe la marca «Volver a ejecutar».
5. Cuando compartes un rol o un Workflow desde la web de Multica, el cliente puede recibirlo en segundo plano y actualizar la lista correspondiente sin volver a importarlo desde el escritorio.

### 4. Administración de roles y rol WeAPM integrado

En «Administración de contexto → Roles» puedes importar directamente uno o varios archivos Markdown, además de importar una carpeta por lotes. Cada lote procesa hasta 5.000 archivos y conserva el motivo de éxito o fallo de cada uno. Los roles personalizados se muestran ahora en una lista completa, sin paginación.

El canal WB incorpora un rol WeAPM integrado y de solo lectura que usa la CLI de WeAPM para consultar Trace y Log e investigar llamadas lentas, errores, tiempos de espera y pilas de excepciones.

> [!NOTE]
> El rol WeAPM solo está disponible en el canal WB y requiere el servicio interno WeAPM, acceso a la red y la CLI `weapm`. Las consultas son de solo lectura.

### 5. Intervención remota por IM y autorización de Feishu

Las integraciones de IM ahora se organizan en tarjetas independientes y plegables, para administrar por separado el estado de conexión, la autorización, la vinculación y las preferencias de notificación. En la autorización de Feishu puedes elegir una aplicación existente o crear una nueva, y cambiarla más adelante. Las respuestas de la intervención remota se mantienen coherentes tras reintentos, reconexiones y callbacks tardíos; un callback que ya no se pueda aplicar aparece como mensaje posterior.

> [!WARNING]
> **Cambiar la aplicación de Feishu elimina la vinculación actual con el chat privado**
> Al sustituir la aplicación se cambia la autorización y se elimina la vinculación existente con el chat privado. Después de autorizar la nueva aplicación, envía otro mensaje privado al bot desde la cuenta correspondiente. Los clientes antiguos no pueden leer directamente los datos de IM actualizados; haz una copia de seguridad de los datos de la aplicación antes de actualizar si necesitas conservar la posibilidad de volver atrás.

### 6. Lienzo de Workflow y configuración de visualización del escritorio

«Configuración → Apariencia» incluye ahora «Zoom de la interfaz», con una escala de toda la aplicación de `50%–250%`. También puedes usar `Ctrl+rueda`, `Ctrl+más/menos` y `Ctrl+0` para ajustarla o restablecerla.

Los gráficos de edición y ejecución de Workflow comparten la preferencia de disposición «Automática / Horizontal / Vertical». El gráfico de ejecución y el editor admiten una vista a pantalla completa que conserva el borrador y el inspector actuales; pulsa `Esc` para salir.

## Mejoras de experiencia

### 1. Encontrar y recuperar conversaciones con más facilidad

La barra lateral incluye ahora una sección de «Conversaciones recientes» que reúne los espacios de trabajo por actividad. Haz clic en una fila para abrir la conversación directamente; si el resultado de búsqueda aún no está cargado, se crea y se muestra la fila correcta en la barra lateral.

Al iniciar una sesión Direct, el cliente sigue esperando a que ACP esté realmente listo en lugar de mostrar un error mientras el Agent sigue iniciándose. Los errores terminales confirmados siguen siendo visibles. Las actualizaciones en segundo plano, la paginación y el cambio de espacio de trabajo ya no permiten que respuestas tardías sobrescriban conversaciones visibles.

### 2. Archivos, adjuntos y estado de entrada más fiables

Los archivos del espacio de trabajo, los artefactos de ejecución, los adjuntos de conversación y las pestañas de archivos de Git comparten acciones como copiar la ruta, mostrar en el administrador de archivos y referenciar en la conversación, conservando el origen original. Las referencias de adjuntos de borradores y colas también se pueden restaurar entre páginas. Al abrir una conversación desde la búsqueda, la barra lateral permanece expandida y centrada en el destino.

La entrada de textos largos, el desplazamiento de conversaciones por IM y las transiciones del espacio de trabajo derecho son más estables. Los errores de inicio de Direct, envío de la cola, vistas previas de adjuntos y referencias a archivos locales quedan limitados a su superficie, en lugar de convertir toda la conversación o el espacio de trabajo en un error.

### 3. Otras mejoras de estabilidad

1. Los gráficos de Workflow se ajustan al área de lienzo correcta al cambiar de dirección o entrar y salir de pantalla completa, reduciendo la superposición entre aristas, nodos y la barra lateral.
2. Las listas de conversaciones recientes y tareas usan paginación limitada y carga progresiva, en lugar de leer todo el historial de una vez.
3. Se han mejorado la recuperación de sesiones Direct, el cierre de intervenciones pendientes y la sincronización de aprobaciones remotas, reduciendo la reaparición de solicitudes ya resueltas y los estados antiguos de las tareas.

---

## 0.21.1: Pequeñas mejoras desde la 0.21.0

### 1. Posponer la configuración inicial

Una vez superadas las comprobaciones del entorno, puedes elegir «Configurar más tarde» cuando la detección de Agents termine o falle, entrar en la aplicación y completar después la configuración del Agent desde Configuración. Puedes continuar incluso sin un Agent disponible. Esta elección y cualquier decisión anterior de omitir Git se conservan tras reiniciar.

### 2. Navegación de la barra lateral y conversaciones recientes más estables

Hemos mejorado la alineación y los estados de selección de la navegación fija, los espacios de trabajo, las conversaciones recientes y el historial de ejecuciones. Las conversaciones recientes ofrecen la misma expansión y las mismas acciones de fila que las listas de espacios de trabajo, y las actualizaciones conservan la selección en la sección actual. Las listas se cargan por páginas mediante «Mostrar más»; al contraer y volver a abrir las listas de espacios de trabajo o de conversaciones recientes, se recupera el primer grupo de elementos.

### 3. Documentación y descargas del sitio actualizadas

El sitio web y las páginas de Demo ahora usan la marca MALING, con una página de lectura de la base de conocimiento y un menú de descargas por plataforma. El cambio de documentos, el diseño para pantallas estrechas y los puntos de entrada de descarga son más estables.
