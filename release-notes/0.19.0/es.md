## Funciones nuevas y modificadas

### 1.Las sesiones Direct reciben mensajes en segundo plano y pueden mantenerse activas

Cuando un Agent sigue trabajando después de terminar una respuesta (por ejemplo, al ejecutarse una tarea programada o un sub-Agent en segundo plano), la sesión Direct ahora sigue recibiendo sus respuestas, llamadas a herramientas, solicitudes de permiso y preguntas, en lugar de perderlas cuando termina la respuesta.

1.Recibes una notificación cuando llega una nueva respuesta en segundo plano; la continuación de la misma respuesta no vuelve a notificar.
2.Mientras el trabajo en segundo plano está en curso, el icono de la sesión en la barra lateral late y un indicador de carga sustituye a la hora; dentro de la sesión puedes ver lo que está pensando el Agent o qué herramienta está llamando.
3.Cuando hay actividad en segundo plano, el campo de mensaje muestra un botón para detener ese trabajo; el envío de mensajes nuevos no se ve afectado.
4.Los archivos modificados por herramientas en segundo plano se incluyen en los cambios de archivos del turno más reciente.

Haz clic derecho en una sesión Direct y elige «Mantener sesión activa» para mostrar una chincheta junto al título; vuelve a hacer clic derecho y elige «Permitir liberar la sesión» para deshacerlo.

Todas las sesiones Direct activas se conservan mientras haya 8 o menos. Por encima de 8, se liberan las sesiones inactivas durante más de 6 horas; por encima de 20, se liberan antes las sesiones que llevan más tiempo sin actividad. Las sesiones mantenidas activas, en ejecución o a la espera de que respondas a un permiso o una pregunta nunca se liberan.

> [!WARNING]
> Mantener una sesión activa solo tiene efecto mientras la aplicación está abierta; las sesiones mantenidas no se inician automáticamente al reiniciar la aplicación. Las tareas programadas de una sesión que no se mantiene activa pueden interrumpirse cuando se libera. Detener el trabajo en segundo plano solo envía una solicitud de cancelación al Agent; es el Agent quien decide si elimina sus tareas programadas.

### 2.Diagramas Mermaid y alertas de GitHub en Markdown

Los bloques de código ` ```mermaid ` de las conversaciones y vistas previas de archivos ahora se muestran como diagramas, con colores que siguen el tema actual. Haz clic en un diagrama para abrirlo en el espacio de trabajo de la derecha, donde puedes ampliarlo, desplazarlo y copiarlo o guardarlo como PNG.

Las conversaciones y vistas previas de archivos también admiten las cinco alertas de GitHub: `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` y `[!CAUTION]`.

### 3.Las imágenes web solo se cargan desde dominios de confianza

Para evitar que documentos o respuestas de Agents hagan que el cliente envíe solicitudes a servidores arbitrarios, las imágenes web en Markdown ahora solo se cargan automáticamente desde dominios de confianza.

1.En las conversaciones: las imágenes que no son de confianza muestran un marcador; haz clic en «Cargar» para confiar en el dominio y mostrar la imagen.
2.En las vistas previas de archivos: las imágenes que no son de confianza se mantienen como enlaces, y un aviso en la parte superior permite confiar a la vez en todos los dominios del documento.
3.Consulta y elimina los dominios de confianza en «Configuración → Avanzado → Dominios de imágenes de confianza».

### 4.Ver resúmenes de compactación de contexto

Cuando un Agent termina de compactar el contexto y devuelve un resumen de texto, el registro de la compactación muestra «Ver resumen». Haz clic para abrir el resumen en el espacio de trabajo de la derecha. Se muestra como Markdown de forma predeterminada, y puedes cambiar al código fuente y copiarlo.

### 5.Obtener la configuración por modelo

Al elegir un modelo que aún no se ha usado en un nodo de workflow, la configuración de AUTO, el campo de mensaje de la página de inicio o una sesión, aparecen «Aún no se ha obtenido la configuración de este modelo» y el botón «Obtener configuración del modelo». Al hacer clic, el Agent se inicia una vez con ese modelo para leer sus opciones reales; no se envía ningún mensaje ni se crea ninguna tarea.

Puedes seguir editando, guardando e iniciando tareas sin hacer clic. Los ajustes de modelo existentes ya no se recortan según la configuración de otro modelo, y un diagnóstico del Agent fallido en segundo plano ya no borra las configuraciones de modelo ya obtenidas.

### 6.Notas de la versión con el estilo de GitHub y disponibles en el menú Ayuda

Las notas del cuadro de diálogo de actualización ahora se muestran igual que en GitHub Releases, con saltos de línea y alertas. La nueva opción «Ayuda → Notas de la versión» permite leer las notas de la versión actual en cualquier momento.

### 7.Nuevo diseño del grafo de workflow

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

### 5.Otras mejoras

1.Vista previa de imágenes unificada: tanto las imágenes de archivos como las de sesiones se amplían directamente con la rueda del ratón, se ajustan a la ventana al abrirse por primera vez y ofrecen un botón «100%» para el tamaño original.
2.Cuando el navegador integrado abre un HTML local, los recursos de directorios superiores ahora se cargan correctamente; si se accede a un directorio fuera del ámbito permitido, puedes elegir «Permitir acceso» en el aviso.
3.Los títulos de las conversaciones tienen una jerarquía más clara y mantienen su espaciado durante el streaming; «Notas de la versión» se movió al final del menú Ayuda.
4.Se corrigió que, con clientes de distintos canales instalados, instalar o actualizar uno cerrara el otro mientras se ejecutaba.
5.Se actualizaron el ACP Registry y el catálogo de Agents.
