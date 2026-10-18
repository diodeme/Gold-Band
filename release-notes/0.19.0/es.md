## Funciones nuevas y modificadas

### 1.Las sesiones Direct reciben mensajes en segundo plano y pueden mantenerse activas

Cuando un Agent sigue trabajando después de terminar una respuesta (por ejemplo, al ejecutarse una tarea programada o un sub-Agent en segundo plano), la sesión Direct ahora sigue recibiendo sus respuestas, llamadas a herramientas, solicitudes de permiso y preguntas, en lugar de perderlas cuando termina la respuesta. (Por ahora, compatible principalmente con Claude Code.)

![image.png](https://static.dion.blue/2026/10/20261010151334594.png)

1.Recibes una notificación cuando llega una nueva respuesta en segundo plano; la continuación de la misma respuesta no vuelve a notificar.
2.Cuando hay actividad en segundo plano, el campo de mensaje muestra un botón para detener ese trabajo; el envío de mensajes nuevos no se ve afectado.

Haz clic derecho en una sesión Direct y elige «Mantener sesión activa» para mostrar una chincheta junto al título; vuelve a hacer clic derecho y elige «Permitir liberar la sesión» para deshacerlo. También puedes activarlo o desactivarlo con el botón de chincheta a la izquierda de «Tramas sin procesar», en la parte superior de la sesión.
![企业微信截图_17916164711387.png](https://static.dion.blue/2026/10/%E4%BC%81%E4%B8%9A%E5%BE%AE%E4%BF%A1%E6%88%AA%E5%9B%BE_17916164711387.png)

![image.png](https://static.dion.blue/2026/10/20261012140125448.png)

Las sesiones Direct, de workflow y de AUTO ahora siguen las mismas reglas de liberación: las sesiones inactivas durante más de 10 minutos se liberan y, cuando hay más de 8 sesiones inactivas, se liberan primero las que llevan más tiempo sin actividad. Las sesiones mantenidas activas, en ejecución o a la espera de que respondas a un permiso o una pregunta nunca se liberan. Si el Agent tiene tareas programadas u otros trabajos largos, mantén la sesión activa.

> [!WARNING]
> 1.Mantener una sesión activa solo tiene efecto mientras la aplicación está abierta; las sesiones mantenidas no se inician automáticamente al reiniciar la aplicación. Las tareas programadas de una sesión que no se mantiene activa pueden interrumpirse cuando se libera. Detener el trabajo en segundo plano solo envía una solicitud de cancelación al Agent; es el Agent quien decide si elimina sus tareas programadas.
> 2.Actualmente ACP no indica si una tarea en segundo plano ha terminado, así que se considera terminada cuando no hay actividad en segundo plano en curso y no se produce actividad nueva durante 10 segundos. Por eso, la sesión puede seguir apareciendo como activa hasta 10 segundos después de que la tarea en segundo plano haya terminado.


### 2.Diagramas Mermaid y alertas de GitHub en Markdown

Los bloques de código ` ```mermaid ` de las conversaciones y vistas previas de archivos ahora se muestran como diagramas, con colores que siguen el tema actual. Haz clic en un diagrama para abrirlo en el espacio de trabajo de la derecha, donde puedes ampliarlo, desplazarlo y copiarlo o guardarlo como PNG.

![image.png](https://static.dion.blue/2026/10/20261010151001399.png)


Las conversaciones y vistas previas de archivos también admiten las cinco alertas de GitHub: `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` y `[!CAUTION]`.

![企业微信截图_17916161364315.png](https://static.dion.blue/2026/10/%E4%BC%81%E4%B8%9A%E5%BE%AE%E4%BF%A1%E6%88%AA%E5%9B%BE_17916161364315.png)


### 3.Las imágenes web solo se cargan desde dominios de confianza

Para evitar que documentos o respuestas de Agents hagan que el cliente envíe solicitudes a servidores arbitrarios, las imágenes web en Markdown ahora solo se cargan automáticamente desde dominios de confianza.

1.En las conversaciones: las imágenes que no son de confianza muestran un marcador; haz clic en «Cargar» para confiar en el dominio y mostrar la imagen.
![企业微信截图_17916165045640.png](https://static.dion.blue/2026/10/%E4%BC%81%E4%B8%9A%E5%BE%AE%E4%BF%A1%E6%88%AA%E5%9B%BE_17916165045640.png)
![企业微信截图_17916165149409.png](https://static.dion.blue/2026/10/%E4%BC%81%E4%B8%9A%E5%BE%AE%E4%BF%A1%E6%88%AA%E5%9B%BE_17916165149409.png)

2.En las vistas previas de archivos: las imágenes que no son de confianza se mantienen como enlaces, y un aviso en la parte superior permite confiar a la vez en todos los dominios del documento.
![企业微信截图_17916165865322.png](https://static.dion.blue/2026/10/%E4%BC%81%E4%B8%9A%E5%BE%AE%E4%BF%A1%E6%88%AA%E5%9B%BE_17916165865322.png)

3.Consulta y elimina los dominios de confianza en «Configuración → Avanzado → Dominios de imágenes de confianza».

![image.png](https://static.dion.blue/2026/10/20261010151155662.png)

### 4.Ver resúmenes de compactación de contexto

Cuando un Agent termina de compactar el contexto y devuelve un resumen de texto, el registro de la compactación muestra «Ver resumen» (solo si el Agent devuelve un resumen). Haz clic para abrir el resumen en el espacio de trabajo de la derecha. Se muestra como Markdown de forma predeterminada, y puedes cambiar al código fuente y copiarlo.
![企业微信截图_17916167016992.png](https://static.dion.blue/2026/10/%E4%BC%81%E4%B8%9A%E5%BE%AE%E4%BF%A1%E6%88%AA%E5%9B%BE_17916167016992.png)


### 5.Obtener la configuración por modelo

Al elegir un modelo que aún no se ha usado en un nodo de workflow, la configuración de AUTO, el campo de mensaje de la página de inicio o una sesión, aparecen «Aún no se ha obtenido la configuración de este modelo» y el botón «Obtener configuración del modelo». Al hacer clic, el Agent se inicia una vez con ese modelo para leer sus opciones reales; no se envía ningún mensaje ni se crea ninguna tarea.

![企业微信截图_17916167706477.png](https://static.dion.blue/2026/10/%E4%BC%81%E4%B8%9A%E5%BE%AE%E4%BF%A1%E6%88%AA%E5%9B%BE_17916167706477.png)


Puedes seguir editando, guardando e iniciando tareas sin hacer clic. Los ajustes no compatibles pasan a no especificados al iniciar la sesión.

### 6.Nuevo diseño del grafo de workflow

El editor de workflow y el grafo de ejecución usan un nuevo diseño automático, con nodos y conexiones mejor organizados. Los grupos creados dinámicamente por AI-DYNAMIC se muestran como marcos anidados, y aparece un indicador de carga cuando el diseño tarda.


## Mejoras de experiencia

### 1.Las entregas de AUTO deben pasar la validación

Cuando AUTO termina el desarrollo, el trabajo pasa a un nodo de validación que lo comprueba frente al requisito original. Si la validación falla, AUTO programa correcciones y vuelve a validar; solo termina cuando la validación se aprueba. La validación toma el requisito original como única base: un plan que excluye contenido del requisito se trata como un bloqueo, y las pruebas existentes modificadas deben volver a verificarse con sus versiones originales.

Los resúmenes largos y las descripciones de tareas que envían los Agents ahora se entregan mediante archivos, lo que reduce los reintentos causados por errores de formato de salida. Cuando AUTO se pausa, el aviso muestra el título concreto del error.

### 2.Actualización de las integraciones con Claude y Codex

Claude ACP se actualiza a `0.87.0` y Codex ACP a `2.1.1`.

1.Las sesiones muestran el estado de la compactación de contexto, su duración y el uso previo a la compactación.
2.La salida de los comandos que ejecuta Codex se sigue acumulando en los detalles de la herramienta, hasta 256.000 caracteres.
3.Las respuestas personalizadas a las preguntas de Claude se envían correctamente.
4.Cuando el Agent necesita que vuelvas a iniciar sesión, se muestra un aviso claro; inicia sesión y vuelve a enviar manualmente.

### 3.Sesiones más estables

1.Al volver a una sesión ya no se muestra repetidamente el estado de carga ni aparece primero contenido antiguo.
2.Los errores de ejecución, como disco lleno o falta de permiso de escritura, se muestran de inmediato y el campo de mensaje ya no se queda esperando; las herramientas sin resultado devuelto muestran «Resultado sin confirmar», y las respuestas y borradores existentes se conservan.
3.Después de detener una respuesta, la sesión siempre termina correctamente el turno actual en lugar de quedarse en «Deteniendo».

### 4.Registro de cambios de archivos más preciso

1.Se corrigió que todos los cambios de un turno se marcaran como incompletos cuando Agents como Claude editaban el mismo archivo varias veces por fragmentos.
2.Se corrigió que los archivos creados por Cursor se registraran como modificaciones, con marcas de Diff mezcladas en su contenido.
3.Las filas de cambios de archivos muestran de forma uniforme el nombre del archivo, el directorio y las líneas añadidas/eliminadas; los archivos editados varias veces también muestran el número de ediciones.

### 5.Menor uso de recursos

1.Las sesiones inactivas se liberan a tiempo según las reglas anteriores, en lugar de mantener procesos del Agent indefinidamente.
2.Las páginas del navegador integrado ocultas durante más de 1 hora se liberan y se recargan desde su dirección al volver a mostrarse.
3.Los Agents npx ya instalados se inician directamente, sin los procesos intermedios de npx que quedaban en ejecución.
4.La barra lateral ya no se vuelve a renderizar por completo mientras una sesión transmite una respuesta.

### 6.Otras mejoras

1.Las tablas Markdown de conversaciones y documentos ocupan el ancho del texto y ajustan las líneas automáticamente; los enlaces largos y el código ya no ensanchan la tabla.
2.Vista previa de imágenes unificada: tanto las imágenes de archivos como las de sesiones se amplían directamente con la rueda del ratón, se ajustan a la ventana al abrirse por primera vez y ofrecen un botón «100%» para el tamaño original.
3.Cuando el navegador integrado abre un HTML local, los recursos de directorios superiores ahora se cargan correctamente; si se accede a un directorio fuera del ámbito permitido, puedes elegir «Permitir acceso» en el aviso.
4.Los títulos de las conversaciones tienen una jerarquía más clara y mantienen su espaciado durante el streaming; «Notas de la versión» se movió al final del menú Ayuda.
5.Se corrigió que, con clientes de distintos canales instalados, instalar o actualizar uno cerrara el otro mientras se ejecutaba.
6.Se actualizaron el ACP Registry y el catálogo de Agents.
