ACP 中完全可以自己实现：

Claude Agent ACP
↓
Custom Tool
AskHuman
↓
ACP Event
↓
前端弹窗
↓
用户选择
↓
submitAnswer
↓
继续执行

从 Agent 视角看：

{
"tool": "AskHuman",
"question": "请选择数据库",
"options": [
"MySQL",
"PostgreSQL",
"MongoDB"
]
}

前端收到后：

┌──────────────────────┐
│ 请选择数据库          │
│                      │
│ ○ MySQL              │
│ ○ PostgreSQL         │
│ ○ MongoDB            │
│                      │
│ [确认]               │
└──────────────────────┘

返回：

{
"selected": "PostgreSQL"
}

Agent继续运行。