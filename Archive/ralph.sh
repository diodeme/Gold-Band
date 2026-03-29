#!/bin/bash
# Ralph Wiggum - 长期运行的 AI 代理循环脚本
# 用法: ./ralph.sh [--tool amp|claude] [最大迭代次数]

set -e # 脚本执行遇到错误时立即退出

# --- 参数解析 ---
TOOL="amp"  # 默认工具设为 amp，保持向后兼容
MAX_ITERATIONS=10 # 默认最大循环次数

while [[ $# -gt 0 ]]; do
  case $1 in
    --tool)
      TOOL="$2"
      shift 2
      ;;
    --tool=*)
      TOOL="${1#*=}"
      shift
      ;;
    *)
      # 如果参数是纯数字，则视为自定义的最大迭代次数
      if [[ "$1" =~ ^[0-9]+$ ]]; then
        MAX_ITERATIONS="$1"
      fi
      shift
      ;;
  esac
done

# 验证工具选择是否合法
if [[ "$TOOL" != "amp" && "$TOOL" != "claude" ]]; then
  echo "错误: 无效的工具 '$TOOL'。必须是 'amp' 或 'claude'。"
  exit 1
fi

# --- 路径与文件初始化 ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PRD_FILE="$SCRIPT_DIR/prd.json"          # 需求文档，存项目状态
PROGRESS_FILE="$SCRIPT_DIR/progress.txt" # 进度日志，记录 AI 做了什么
ARCHIVE_DIR="$SCRIPT_DIR/archive"        # 归档目录，存放历史运行记录
LAST_BRANCH_FILE="$SCRIPT_DIR/.last-branch" # 记录上一次运行的 Git 分支/任务分支

# --- 自动归档逻辑 ---
# 如果分支发生了切换，说明开始了新任务，自动备份旧任务的文件
if [ -f "$PRD_FILE" ] && [ -f "$LAST_BRANCH_FILE" ]; then
  # 从 JSON 中提取当前分支名
  CURRENT_BRANCH=$(jq -r '.branchName // empty' "$PRD_FILE" 2>/dev/null || echo "")
  LAST_BRANCH=$(cat "$LAST_BRANCH_FILE" 2>/dev/null || echo "")
  
  if [ -n "$CURRENT_BRANCH" ] && [ -n "$LAST_BRANCH" ] && [ "$CURRENT_BRANCH" != "$LAST_BRANCH" ]; then
    echo "检测到分支变更，正在归档上一轮运行数据: $LAST_BRANCH"
    DATE=$(date +%Y-%m-%d)
    # 去掉分支名前缀 ralph/ 方便建立文件夹
    FOLDER_NAME=$(echo "$LAST_BRANCH" | sed 's|^ralph/||')
    ARCHIVE_FOLDER="$ARCHIVE_DIR/$DATE-$FOLDER_NAME"
    
    mkdir -p "$ARCHIVE_FOLDER"
    [ -f "$PRD_FILE" ] && cp "$PRD_FILE" "$ARCHIVE_FOLDER/"
    [ -f "$PROGRESS_FILE" ] && cp "$PROGRESS_FILE" "$ARCHIVE_FOLDER/"
    echo "   已归档至: $ARCHIVE_FOLDER"
    
    # 重置进度文件，准备新一轮任务
    echo "# Ralph 进度日志" > "$PROGRESS_FILE"
    echo "开始时间: $(date)" >> "$PROGRESS_FILE"
    echo "---" >> "$PROGRESS_FILE"
  fi
fi

# 更新当前记录的分支名
if [ -f "$PRD_FILE" ]; then
  CURRENT_BRANCH=$(jq -r '.branchName // empty' "$PRD_FILE" 2>/dev/null || echo "")
  if [ -n "$CURRENT_BRANCH" ]; then
    echo "$CURRENT_BRANCH" > "$LAST_BRANCH_FILE"
  fi
fi

# 如果进度文件不存在，则创建一个初始化的
if [ ! -f "$PROGRESS_FILE" ]; then
  echo "# Ralph 进度日志" > "$PROGRESS_FILE"
  echo "开始时间: $(date)" >> "$PROGRESS_FILE"
  echo "---" >> "$PROGRESS_FILE"
fi

echo "启动 Ralph - 使用工具: $TOOL - 最大迭代次数: $MAX_ITERATIONS"

# --- 核心任务循环 ---
for i in $(seq 1 $MAX_ITERATIONS); do
  echo ""
  echo "==============================================================="
  echo "  Ralph 迭代中 $i / $MAX_ITERATIONS (当前工具: $TOOL)"
  echo "==============================================================="

  # 根据选择的工具，读取对应的提示词文件并运行 AI
  if [[ "$TOOL" == "amp" ]]; then
    # amp 模式：允许 AI 自动执行所有危险操作（如改写代码）
    OUTPUT=$(cat "$SCRIPT_DIR/prompt.md" | amp --dangerously-allow-all 2>&1 | tee /dev/stderr) || true
  else
    # Claude Code 模式：跳过权限申请，直接在后台运行
    OUTPUT=$(claude --dangerously-skip-permissions --print < "$SCRIPT_DIR/CLAUDE.md" 2>&1 | tee /dev/stderr) || true
  fi
  
  # 检查 AI 输出中是否包含特定的“完成信号”
  # 只要 AI 输出中带有 <promise>COMPLETE</promise>，就代表任务全部达成
  if echo "$OUTPUT" | grep -q "<promise>COMPLETE</promise>"; then
    echo ""
    echo "Ralph 已成功完成所有任务！"
    echo "在第 $i 次迭代时提前结束。"
    exit 0
  fi
  
  echo "第 $i 次迭代结束。继续下一步..."
  sleep 2 # 稍微等待，防止 API 调用频率过快
done

echo ""
echo "Ralph 达到了最大迭代次数 ($MAX_ITERATIONS) 但未检测到任务完成标志。"
echo "请检查 $PROGRESS_FILE 以获取最新状态。"
exit 1