使用者提供了以下檔案，內容未隨訊息傳送，需要時依路徑讀取：
{% for file in files %}- `{{ file.path }}`（{{ file.size }} 位元組）
{% endfor %}