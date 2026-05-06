use gold_band::config::DesktopLanguage;

pub struct Translator {
    language: DesktopLanguage,
}

impl Translator {
    pub fn new(language: DesktopLanguage) -> Self {
        Self { language }
    }

    pub fn tr(&self, key: &str) -> String {
        match self.language {
            DesktopLanguage::En => en(key),
            DesktopLanguage::ZhCn => zh_cn(key),
        }
        .unwrap_or(key)
        .to_string()
    }

    pub fn format(&self, key: &str, value: &str) -> String {
        self.tr(key).replace("{value}", value)
    }

    pub fn format_pair(&self, key: &str, value: &str) -> String {
        self.tr(key).replace("{value}", value)
    }
}

fn zh_cn(key: &str) -> Option<&'static str> {
    Some(match key {
        "summary.all" => "全部任务",
        "summary.running" => "运行中",
        "summary.resumable" => "可恢复",
        "summary.failed" => "失败",
        "summary.invalid" => "配置异常",
        "stream.requirement" => "需求",
        "stream.round" => "轮次 {value}",
        "stream.runEvents" => "运行事件",
        "stream.node" => "节点 {value}",
        "stream.artifact" => "产物 {value}",
        "stream.attachment" => "附件 {value}",
        "stream.progressEvents" => "进度事件",
        "stream.field.status" => "状态：{value}",
        "stream.field.outcome" => "结果：{value}",
        "stream.field.trigger" => "触发：{value}",
        "stream.field.repairLoops" => "修复循环：{value}",
        "stream.field.currentNode" => "当前节点：{value}",
        "stream.field.attempt" => "Attempt：{value}",
        "stream.field.startedAt" => "开始时间：{value}",
        "stream.field.finishedAt" => "结束时间：{value}",
        "detail.round" => "轮次 {value}",
        "detail.requirement" => "需求",
        "detail.node" => "节点 {value}",
        "detail.artifact" => "产物 {value}",
        "detail.attachment" => "附件 {value}",
        "detail.workerRef" => "Worker 会话 {value}",
        "detail.runEvents" => "运行事件",
        "detail.runtimeLog" => "运行时日志",
        "fallback.missingRequirement" => "未找到 requirement.md",
        "fallback.missingWorkerRef" => "未找到 worker-ref",
        "fallback.missingEvents" => "未找到 events",
        "fallback.missingRuntimeLog" => "未找到 runtime log",
        _ => return None,
    })
}

fn en(key: &str) -> Option<&'static str> {
    Some(match key {
        "summary.all" => "All Tasks",
        "summary.running" => "Running",
        "summary.resumable" => "Resumable",
        "summary.failed" => "Failed",
        "summary.invalid" => "Config Issues",
        "stream.requirement" => "Requirement",
        "stream.round" => "Round {value}",
        "stream.runEvents" => "Run Events",
        "stream.node" => "Node {value}",
        "stream.artifact" => "Artifact {value}",
        "stream.attachment" => "Attachment {value}",
        "stream.progressEvents" => "Progress Events",
        "stream.field.status" => "Status: {value}",
        "stream.field.outcome" => "Outcome: {value}",
        "stream.field.trigger" => "Trigger: {value}",
        "stream.field.repairLoops" => "Repair loops: {value}",
        "stream.field.currentNode" => "Current node: {value}",
        "stream.field.attempt" => "Attempt: {value}",
        "stream.field.startedAt" => "Started at: {value}",
        "stream.field.finishedAt" => "Finished at: {value}",
        "detail.round" => "Round {value}",
        "detail.requirement" => "Requirement",
        "detail.node" => "Node {value}",
        "detail.artifact" => "Artifact {value}",
        "detail.attachment" => "Attachment {value}",
        "detail.workerRef" => "Worker Ref {value}",
        "detail.runEvents" => "Run Events",
        "detail.runtimeLog" => "Runtime Log",
        "fallback.missingRequirement" => "requirement.md not found",
        "fallback.missingWorkerRef" => "worker-ref not found",
        "fallback.missingEvents" => "events not found",
        "fallback.missingRuntimeLog" => "runtime log not found",
        _ => return None,
    })
}
