## Funciones nuevas y modificadas

### 1.Intervención remota desde Feishu / Lark

Además de WeCom, ahora también puedes atender desde Feishu / Lark las solicitudes de permiso, preguntas de seguimiento y comprobaciones manuales que bloquean una tarea, y recibir notificaciones de Run y de fin de turno.

1. Abre "Configuración → General → Intervención remota y notificaciones de IM", despliega "Feishu / Lark" y haz clic en "Escanear para conectar".
2. Revisa los permisos necesarios, haz clic en "Aceptar y generar código QR" y escanea el código con Feishu / Lark en tu teléfono. Puedes elegir una app de Feishu / Lark existente o crear una nueva.
3. Tras la autorización, envía un mensaje al bot en Feishu / Lark para vincular el chat privado individual.
4. Cuando una tarea te necesite, Feishu / Lark recibe una tarjeta interactiva con el ID, el nodo, la tarea y el espacio de trabajo. Desde ella puedes permitir o denegar permisos, responder preguntas de seguimiento (opción única, opción múltiple, respuestas personalizadas y texto) y marcar comprobaciones manuales como superadas o fallidas.

Tras enviarla, la tarjeta conserva tus respuestas y desactiva los controles. En las solicitudes ya atendidas en el escritorio o en WeCom, la tarjeta muestra dónde se atendieron. Las solicitudes que terminaron por una detención, un tiempo de espera agotado u otros motivos similares se muestran como "Cancelada", nunca como rechazada. Si un envío tarda en procesarse y finalmente no se acepta, el bot envía un mensaje aparte con el motivo.

En "Menú de configuración" puedes volver a vincular la cuenta receptora, cambiar la app de Feishu / Lark, volver a autorizar o eliminar la configuración.

> [!NOTE]
> Un bot se conecta a un solo dispositivo. Por ahora solo se admiten chats privados individuales, y una tarjeta de Feishu / Lark admite como máximo 5 preguntas de seguimiento; atiende las solicitudes más grandes en el escritorio. El permiso para leer tu nombre es opcional; sin él, la cuenta receptora se muestra como "Chat privado vinculado".

> [!WARNING]
> Tras la actualización, los datos de IM se migran a un nuevo formato que las versiones anteriores del cliente ya no pueden leer; para volver a una versión anterior hay que restaurar antes una copia de seguridad. Las tarjetas de WeCom enviadas antes de la actualización y aún pendientes dejan de ser válidas; atiende esas solicitudes en el escritorio.

### 2.Enviar ahora y Envío automático para los mensajes pendientes

Ahora tienes más control sobre cuándo se envían los mensajes puestos en cola mientras el Agent responde.

1. "Enviar ahora" envía un mensaje pendiente de inmediato. Si hay una respuesta en curso, primero se te pregunta si quieres detenerla; después de marcar "No volver a preguntar", puedes reactivar la confirmación en "Configuración → Avanzado → Mensajes pendientes".
2. Interruptor "Envío automático": activado, los mensajes pendientes se envían uno a uno al terminar cada respuesta; desactivado, se quedan en la cola hasta que envíes cada uno con "Enviar ahora".
3. Cuando la sesión se detiene o el envío anterior falla, la cola indica que está en pausa y el motivo; haz clic en "Reanudar" para seguir enviando automáticamente.
4. Vacía la cola de pendientes desde "Más acciones".

### 3.Borradores

La barra de herramientas del cuadro de entrada incluye ahora "Borradores". Haz clic en "Guardar en borradores" para apartar lo que has escrito y, más tarde, en "Mover a la entrada" para seguir editándolo. Los borradores se guardan solo en la sesión actual, nunca se envían automáticamente y cada sesión conserva hasta 20.

### 4.Sub-Agents nativos de Claude

Con Claude ACP `0.90.0` o posterior, Gold Band lee los estados de los sub-Agents que informa Claude y muestra con precisión en ejecución, esperando interacción, completado, interrumpido y con error. Los sub-Agents ya no aparecen como interrumpidos solo porque haya terminado el turno principal.

1. Mientras haya sub-Agents en ejecución, aparece un resumen de ellos sobre el cuadro de entrada; haz clic en uno para ver su registro completo en el espacio de trabajo derecho.
2. Cuando termina el turno principal, el cuadro de entrada mantiene el botón "Detener" mientras algún sub-Agent siga ejecutándose en segundo plano o esperando un permiso.
3. Cuando una sesión tiene trabajo en segundo plano, la información emergente del uso de Token indica "No incluye el uso de activaciones en segundo plano".

> [!IMPORTANT]
> Claude ACP se ha actualizado a `0.90.0`. Si tu versión es demasiado antigua, se te pedirá que la actualices; Gold Band no modifica tu configuración de instalación automáticamente.

### 5.Confirmar el destino antes de hacer push

Al hacer push de una rama en el control de código fuente, primero se muestran la rama de origen, el repositorio remoto de destino y la rama de destino, y el push solo se realiza tras confirmarlo. En una rama que se publica por primera vez, puedes elegir el repositorio remoto y establecer la rama de destino como upstream cuando la publicación tenga éxito. Las ramas sin upstream se marcan como `new`.

### 6.Abrir la búsqueda de sesiones con Ctrl+K

Pulsa `Ctrl+K` (`Command+K` en macOS) para abrir la búsqueda de sesiones; si ya está abierta, al pulsarlo de nuevo se selecciona la palabra clave, y `Esc` la cierra. `Ctrl+F` queda reservado para la búsqueda dentro de archivos en los editores.


## Mejoras de experiencia

### 1.Página de configuración y tarjetas de IM

1. "Intervención remota y notificaciones de IM" muestra ahora WeCom y Feishu / Lark como dos tarjetas plegables, con los tipos de notificación agrupados en "Requiere tu acción" y "Resultados de ejecución".
2. Las tarjetas de WeCom y Feishu / Lark siguen el idioma del escritorio y admiten los siete idiomas de la interfaz.
3. Las tarjetas de WeCom ya atendidas muestran todas las preguntas y respuestas personalizadas. Las tarjetas de WeCom de opción única con varias preguntas admiten como máximo 3; atiende las solicitudes más grandes en el escritorio.

### 2.Sesiones más estables

1. Cuando un Agent falla durante el arranque, los reintentos automáticos vuelven a ejecutarse correctamente; los turnos que fallan después de que el mensaje ya llegó al Agent pasan a recuperación manual para evitar ejecuciones duplicadas. Durante el arranque, el cuadro de entrada muestra "Preparando Agent".
2. Si no se puede crear una sesión, se muestra directamente el motivo del fallo en lugar de seguir cargando.
3. Las preguntas y solicitudes de permiso que quedan tras un cierre inesperado del cliente dejan de ser válidas; al responderlas se muestra un mensaje claro en lugar de indicar que se enviaron correctamente.
4. Al continuar una sesión se mantienen su modelo y su modo de permisos originales.
5. Corregido el uso de Token contado dos veces al continuar una sesión Direct después de haber sido reclamada.

### 3.Desplazamiento más natural

1. La lista de mensajes sigue anclada al final cuando el cuadro de entrada crece.
2. Mientras lees el historial, el contenido nuevo ya no te devuelve al final; al seguir desplazándote hacia arriba desde el principio se carga el historial anterior, incluso cuando el contenido ocupa menos de una pantalla.
3. La posición de desplazamiento del cuadro de entrada ya no salta al escribir borradores largos.

### 4.Otras mejoras

1. Los menús `/` y `@` del cuadro de entrada vuelven a leer las skills y los roles cada vez que se abren, así que las skills nuevas se pueden usar sin reiniciar.
2. Los adjuntos con el mismo nombre ya no sobrescriben adjuntos anteriores de la conversación; cuando el directorio de datos de la sesión está anidado a demasiada profundidad para guardar un adjunto, se muestra un mensaje claro.
3. Las imágenes locales de las conversaciones se resuelven respecto al espacio de trabajo de la sesión; el navegador integrado conserva los parámetros de consulta y los anclajes al abrir enlaces HTML locales.
4. La vista previa en vivo de Markdown ocupa todo el ancho del panel; el tamaño de fuente predeterminado del editor pasa a ser 13px.
5. Cuando falla una pestaña del espacio de trabajo derecho, solo esa pestaña muestra "No se pudo mostrar esta pestaña"; las demás no se ven afectadas.
6. Los servidores MCP iniciados con `npx` o `uvx` ahora esperan hasta 180 segundos a que se descarguen sus dependencias en la primera comprobación, en lugar de fallar mientras aún se descargan.
7. Cuando el idioma del sistema es chino, el idioma de la interfaz se elige según la etiqueta de escritura simplificada o tradicional, sin verse afectado por la región.
8. Corregida la aparición espontánea de información emergente tras hacer clic en diálogos o menús.
9. Actualizados el ACP Registry y el catálogo de Agents.
