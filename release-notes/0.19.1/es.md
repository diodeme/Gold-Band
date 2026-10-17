## Mejoras de experiencia

### 1.Correcciones al detener en segundo plano en Direct

1.Corregido que Detener no hiciera nada cuando aparecía una pregunta en segundo plano después de que una tarea programada despertaba al Agent. Detener ahora actúa correctamente sobre el trabajo en segundo plano: una solicitud de permiso pendiente se registra como cancelada, a una pregunta pendiente se responde con «cancelar», y la sesión ya no se queda atascada esperando.
2.Las solicitudes de permiso y preguntas que llegan mientras se ejecuta una respuesta ahora se resuelven con ese turno, en lugar de quedarse en la sesión después de detener; las interacciones ya procesadas ya no vuelven al estado pendente por contenido en vivo más reciente.

### 2.La actividad en segundo plano ya no se queda atascada

Algunos Agents no envían la actualización final de las herramientas después de ser detenidos, lo que mantenía la sesión como activa en la barra lateral, dejaba el botón de detener siempre visible e impedía que las sesiones mantenidas activas se liberaran. Ahora las herramientas se liquidan cuando termina el turno al que pertenecen o cuando se detiene el trabajo en segundo plano, y la sesión vuelve a inactiva tras 10 segundos sin actividad.

### 3.Corrección del menú de modelo en paneles estrechos

Cuando el panel de conversación es estrecho, el submenú del modelo y del razonamiento se comprimía hasta el borde izquierdo de la ventana y cubría la barra lateral. Sin espacio, ahora se despliega dentro del propio menú principal (con clic o las teclas de flecha); con espacio suficiente, sigue abriéndose al pasar el ratón, como antes.

### 4.Otras mejoras

1.Actualizados el snapshot del ACP Registry y el catálogo de Agents.
