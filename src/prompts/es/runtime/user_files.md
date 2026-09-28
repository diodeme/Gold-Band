El usuario proporcionó los siguientes archivos. Su contenido no se incluye en este mensaje; léelos por su ruta cuando sea necesario:
{% for file in files %}- `{{ file.path }}` ({{ file.size }} bytes)
{% endfor %}