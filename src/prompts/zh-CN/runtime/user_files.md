用户提供了以下文件，内容未随消息发送，需要时按路径读取：
{% for file in files %}- `{{ file.path }}`（{{ file.size }} 字节）
{% endfor %}