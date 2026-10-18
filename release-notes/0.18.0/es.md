## Funciones nuevas y modificadas

### 1.Gestión directa de archivos en el espacio de trabajo

Ahora puedes crear archivos y carpetas, cambiar sus nombres y eliminarlos directamente desde el árbol de archivos del espacio de trabajo derecho, sin cambiar al administrador de archivos del sistema.

La creación y el cambio de nombre se realizan en el propio árbol. Antes de eliminar se solicita confirmación y el contenido se mueve a la Papelera del sistema. Cuando el árbol de archivos tenga el foco, pulsa `Ctrl+Z` (`Command+Z` en macOS) para deshacer la creación, el cambio de nombre o la eliminación más reciente.

![image.png](https://static.dion.blue/2026/09/20260930162214313.png)

### 2.Citas directas de archivos y Diffs en una conversación

El menú `@` del cuadro de mensaje ahora permite explorar y buscar archivos en el espacio de trabajo actual. Al seleccionar un archivo, se añade una referencia al mensaje para el Agent actual; los nodos posteriores de Workflow y AUTO también reciben estas referencias.

![image.png](https://static.dion.blue/2026/09/20260930163805780.png)

Selecciona texto en un archivo del espacio de trabajo, un directorio de ejecución, un adjunto o un cambio de archivo para citar las líneas correspondientes. Al seleccionar contenido en un Diff se incluyen automáticamente los bloques de cambios completos relacionados, y desde el encabezado del archivo se puede citar el Diff completo. Las citas conservan la ruta, los números de línea y el origen del cambio para que el Agent interprete el contexto con precisión.

![image.png](https://static.dion.blue/2026/09/20260930162523931.png)

### 3.Descartar los cambios de un archivo desde el control de código fuente

Las acciones de archivo del control de código fuente ahora incluyen “Descartar cambios”. Después de confirmar, tanto los cambios preparados como los no preparados se restauran a `HEAD`; los archivos nuevos que Git todavía no rastrea se eliminan.

> [!attention] Los cambios descartados no se pueden recuperar
> Esta acción no mueve el contenido a la Papelera del sistema. Confirma que ya no necesitas los cambios locales del archivo antes de continuar.

![image.png](https://static.dion.blue/2026/09/20260930162543785.png)

### 4.Nuevo acceso a las actualizaciones y nuevo flujo de instalación

Cuando hay una versión nueva, aparece un botón persistente “Actualizar” en la barra de título. Desde ahí puedes leer las notas de la versión, ampliar sus imágenes e iniciar la actualización directamente. El diálogo muestra el progreso de la descarga, permite reintentar si falla y solicita reiniciar para instalar cuando termina.

![image.png](https://static.dion.blue/2026/09/20260930162647062.png)

La instalación ahora guarda las ediciones abiertas, pausa las sesiones y cierra las conexiones y los servicios en segundo plano mediante el flujo normal de salida. Después, Gold Band verifica la firma y la versión de la actualización descargada antes de instalarla, lo que reduce el riesgo de perder el estado del trabajo o instalar un paquete incorrecto.

Esta versión es una actualización normal: el cliente solo avisa de la nueva versión y espera a que inicies manualmente la descarga y la instalación.

### 5.Reparación selectiva de la caché de npx en la gestión de Agents

Cuando el diagnóstico de un Agent npm/npx falla porque su caché de instalación está incompleta, la gestión de Agents ofrece “Reparar caché”. Tras revisar los directorios de destino, Gold Band elimina únicamente los directorios de instalación de npx relacionados con ese error y vuelve a comprobar el Agent automáticamente.

La reparación no vacía toda la caché de npm ni elimina archivos del proyecto, paquetes globales o ajustes de la cuenta. Los Agents que estén usando esos directorios de caché pueden interrumpirse, por lo que conviene detener primero las tareas relacionadas.

![image.png|500](https://static.dion.blue/2026/09/20260930162937093.png)
![image.png|500](https://static.dion.blue/2026/09/20260930162952757.png)

## Mejoras de experiencia

### 1.El espacio de trabajo derecho sigue el worktree de la sesión

La exploración, edición y búsqueda de archivos, los enlaces de archivos, el menú de archivos `@` y el control de código fuente ahora usan de forma coherente el worktree real de la sesión actual. Al cambiar de nodo o worktree, el espacio de trabajo derecho también cambia, sin leer por error el directorio principal del proyecto.

Si el worktree de una sesión anterior ya se retiró, Gold Band muestra claramente que no está disponible. Solo cambia al espacio de trabajo principal cuando eliges “Explorar el espacio de trabajo principal”, sin recurrir a él de forma silenciosa.

### 2.Optimización del control de código fuente para abrirlo más rápido

La carga inicial, las actualizaciones en segundo plano y la revisión de commits son ahora más ágiles, lo que reduce la espera al abrir el control de código fuente por primera vez. La información del repositorio y la lista de archivos aparecen antes, y las estadísticas de líneas se completan después; la búsqueda de archivos omite los metadatos de `.git`, y los archivos de bloqueo temporales de Git ya no provocan actualizaciones completas innecesarias.

También se corrigieron los retrasos al actualizar el estado después de cambios en archivos y las solicitudes lentas que sobrescribían resultados más recientes. Los Diffs abiertos, los términos de búsqueda y la información del repositorio permanecen disponibles durante la reconciliación en segundo plano.

### 3.Otras mejoras

1.El espacio de trabajo de archivos ahora muestra vistas previas nativas de SVG y puede abrir el archivo actual con una aplicación del sistema.
2.Los registros de cambios de archivos de cada turno del Agent son más completos, incluso con procesamiento en segundo plano o cambios fuera del worktree; las pruebas incompletas se identifican claramente.
3.Al iniciarse, la aplicación carga únicamente el idioma activo de la interfaz para reducir lecturas y trabajo innecesarios.
4.Se corrigió la desaparición de los bordes de la ventana después del cambio de tamaño nativo en Windows 10.
5.Se corrigieron los tooltips de texto truncado que permanecían abiertos después de acciones de menú, los tooltips que quedaban fijados en la esquina superior izquierda al ocultarse su activador y la pérdida de foco del cuadro de mensaje tras quitar una etiqueta de rol.
6.El canal predeterminado ya no muestra la entrada no disponible de Gestión de requisitos y se corrigieron varios errores de traducción restantes.
