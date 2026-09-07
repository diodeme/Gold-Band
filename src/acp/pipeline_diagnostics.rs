use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::acp::connection::SessionQueueHighWatermarks;

pub(crate) const PIPELINE_DETAILED_WINDOW: Duration = Duration::from_secs(5);
const SEVERE_QUEUE_WAIT: Duration = Duration::from_secs(1);
const QUEUE_WAIT_ANOMALY_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipelineUpdateKind {
    AgentText,
    AgentThought,
    ToolCall,
    ToolCallUpdate,
    Plan,
    Catalog,
    Usage,
    Other,
}

impl PipelineUpdateKind {
    pub(crate) fn from_frame(frame: &Value) -> Self {
        let kind = frame
            .pointer("/params/update/sessionUpdate")
            .or_else(|| frame.pointer("/params/update/session_update"))
            .or_else(|| frame.pointer("/params/sessionUpdate"))
            .or_else(|| frame.pointer("/params/session_update"))
            .and_then(Value::as_str);
        match kind {
            Some("agent_message_chunk") => Self::AgentText,
            Some("agent_thought_chunk") => Self::AgentThought,
            Some("tool_call") => Self::ToolCall,
            Some("tool_call_update") => Self::ToolCallUpdate,
            Some("plan") | Some("plan_update") => Self::Plan,
            Some("usage_update") => Self::Usage,
            Some(
                "available_commands_update"
                | "current_mode_update"
                | "config_option_update"
                | "session_info_update",
            ) => Self::Catalog,
            _ => Self::Other,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::AgentText => 0,
            Self::AgentThought => 1,
            Self::ToolCall => 2,
            Self::ToolCallUpdate => 3,
            Self::Plan => 4,
            Self::Catalog => 5,
            Self::Usage => 6,
            Self::Other => 7,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct DurationMetric {
    count: u64,
    total_micros: u64,
    max_micros: u64,
}

impl DurationMetric {
    fn observe(&mut self, duration: Duration) {
        let micros = duration.as_micros().min(u128::from(u64::MAX)) as u64;
        self.count = self.count.saturating_add(1);
        self.total_micros = self.total_micros.saturating_add(micros);
        self.max_micros = self.max_micros.max(micros);
    }

    fn as_json(self) -> Value {
        json!({
            "count": self.count,
            "totalMs": self.total_micros / 1_000,
            "maxMs": self.max_micros / 1_000,
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct PipelineMetrics {
    received_frames: u64,
    dequeued_frames: u64,
    processed_frames: u64,
    received_bytes: u64,
    last_route_sequence: u64,
    queue_wait_buckets: [u64; 8],
    process_buckets: [u64; 8],
    queue_wait: DurationMetric,
    process: DurationMetric,
    raw_append: DurationMetric,
    raw_roll: DurationMetric,
    timeline_upsert: DurationMetric,
    timeline_compaction: DurationMetric,
    live_emit: DurationMetric,
    update_kinds: [u64; 8],
}

impl PipelineMetrics {
    fn observe_frame(
        &mut self,
        bytes: usize,
        sequence: u64,
        queue_wait: Duration,
        kind: PipelineUpdateKind,
    ) {
        self.received_frames = self.received_frames.saturating_add(1);
        self.dequeued_frames = self.dequeued_frames.saturating_add(1);
        self.received_bytes = self.received_bytes.saturating_add(bytes as u64);
        self.last_route_sequence = self.last_route_sequence.max(sequence);
        self.queue_wait.observe(queue_wait);
        self.queue_wait_buckets[latency_bucket(queue_wait)] =
            self.queue_wait_buckets[latency_bucket(queue_wait)].saturating_add(1);
        self.update_kinds[kind.index()] = self.update_kinds[kind.index()].saturating_add(1);
    }

    fn observe_processed(&mut self, elapsed: Duration) {
        self.processed_frames = self.processed_frames.saturating_add(1);
        self.process.observe(elapsed);
        self.process_buckets[latency_bucket(elapsed)] =
            self.process_buckets[latency_bucket(elapsed)].saturating_add(1);
    }

    fn as_json(self) -> Value {
        json!({
            "receivedFrames": self.received_frames,
            "dequeuedFrames": self.dequeued_frames,
            "processedFrames": self.processed_frames,
            "receivedBytes": self.received_bytes,
            "lastRouteSequence": self.last_route_sequence,
            "latencyBucketUpperBoundsMs": [1, 10, 50, 100, 500, 1_000, 5_000],
            "queueWaitBuckets": buckets_json(self.queue_wait_buckets),
            "queueWait": self.queue_wait.as_json(),
            "processBuckets": buckets_json(self.process_buckets),
            "process": self.process.as_json(),
            "rawAppend": self.raw_append.as_json(),
            "rawRoll": self.raw_roll.as_json(),
            "timelineUpsert": self.timeline_upsert.as_json(),
            "timelineCompaction": self.timeline_compaction.as_json(),
            "liveEmit": self.live_emit.as_json(),
            "updateKindCounts": {
                "agentText": self.update_kinds[0],
                "agentThought": self.update_kinds[1],
                "toolCall": self.update_kinds[2],
                "toolCallUpdate": self.update_kinds[3],
                "plan": self.update_kinds[4],
                "catalog": self.update_kinds[5],
                "usage": self.update_kinds[6],
                "other": self.update_kinds[7],
            },
        })
    }
}

#[derive(Debug)]
pub(crate) struct AcpPipelineDiagnostics {
    prompt_started_at: Instant,
    window_started_at: Instant,
    detailed: bool,
    route_generation: u64,
    total: PipelineMetrics,
    window: PipelineMetrics,
    pending_queue_wait_anomaly: Option<Duration>,
    last_queue_wait_anomaly_at: Option<Instant>,
}

impl AcpPipelineDiagnostics {
    pub(crate) fn new(started_at: Instant, detailed: bool, route_generation: u64) -> Self {
        Self {
            prompt_started_at: started_at,
            window_started_at: started_at,
            detailed,
            route_generation,
            total: PipelineMetrics::default(),
            window: PipelineMetrics::default(),
            pending_queue_wait_anomaly: None,
            last_queue_wait_anomaly_at: None,
        }
    }

    pub(crate) fn observe_frame(
        &mut self,
        bytes: usize,
        sequence: u64,
        queue_wait: Duration,
        kind: PipelineUpdateKind,
    ) {
        self.total.observe_frame(bytes, sequence, queue_wait, kind);
        self.window.observe_frame(bytes, sequence, queue_wait, kind);
        if queue_wait >= SEVERE_QUEUE_WAIT {
            self.pending_queue_wait_anomaly = Some(
                self.pending_queue_wait_anomaly
                    .map(|pending| pending.max(queue_wait))
                    .unwrap_or(queue_wait),
            );
        }
    }

    pub(crate) fn observe_processed(&mut self, elapsed: Duration) {
        self.total.observe_processed(elapsed);
        self.window.observe_processed(elapsed);
    }

    pub(crate) fn observe_raw_append(&mut self, elapsed: Duration, roll: Option<Duration>) {
        self.total.raw_append.observe(elapsed);
        self.window.raw_append.observe(elapsed);
        if let Some(roll) = roll {
            self.total.raw_roll.observe(roll);
            self.window.raw_roll.observe(roll);
        }
    }

    pub(crate) fn observe_timeline_upsert(
        &mut self,
        elapsed: Duration,
        compaction_elapsed: Option<Duration>,
    ) {
        self.total.timeline_upsert.observe(elapsed);
        self.window.timeline_upsert.observe(elapsed);
        if let Some(compaction_elapsed) = compaction_elapsed {
            self.total.timeline_compaction.observe(compaction_elapsed);
            self.window.timeline_compaction.observe(compaction_elapsed);
        }
    }

    pub(crate) fn observe_live_emit(&mut self, elapsed: Duration) {
        self.total.live_emit.observe(elapsed);
        self.window.live_emit.observe(elapsed);
    }

    pub(crate) fn take_detailed_window(&mut self, now: Instant) -> Option<Value> {
        if !self.detailed
            || now.saturating_duration_since(self.window_started_at) < PIPELINE_DETAILED_WINDOW
        {
            return None;
        }
        let elapsed = now.saturating_duration_since(self.window_started_at);
        let metrics = std::mem::take(&mut self.window);
        self.window_started_at = now;
        Some(with_common_fields(
            metrics.as_json(),
            "acp_pipeline_window",
            self.route_generation,
            elapsed,
        ))
    }

    pub(crate) fn take_queue_wait_anomaly(&mut self, now: Instant) -> Option<Value> {
        let wait = self.pending_queue_wait_anomaly?;
        if self
            .last_queue_wait_anomaly_at
            .is_some_and(|last| now.saturating_duration_since(last) < QUEUE_WAIT_ANOMALY_INTERVAL)
        {
            return None;
        }
        self.pending_queue_wait_anomaly = None;
        self.last_queue_wait_anomaly_at = Some(now);
        Some(json!({
            "event": "acp_pipeline_queue_wait_anomaly",
            "routeGeneration": self.route_generation,
            "queueWaitMs": duration_ms(wait),
            "thresholdMs": duration_ms(SEVERE_QUEUE_WAIT),
            "promptElapsedMs": duration_ms(now.saturating_duration_since(self.prompt_started_at)),
        }))
    }

    pub(crate) fn finish(
        self,
        now: Instant,
        status: &str,
        queue: SessionQueueHighWatermarks,
    ) -> Value {
        let prompt_elapsed = now.saturating_duration_since(self.prompt_started_at);
        let mut value = with_common_fields(
            self.total.as_json(),
            "acp_pipeline_summary",
            self.route_generation,
            prompt_elapsed,
        );
        if let Some(object) = value.as_object_mut() {
            object.insert("status".to_string(), Value::String(status.to_string()));
            object.insert(
                "promptElapsedMs".to_string(),
                Value::from(duration_ms(prompt_elapsed)),
            );
            object.insert(
                "queueHighWatermarks".to_string(),
                json!({
                    "ingressFrames": queue.ingress_frames,
                    "ingressBytes": queue.ingress_bytes,
                    "pumpFrames": queue.pump_frames,
                    "pumpBytes": queue.pump_bytes,
                }),
            );
        }
        value
    }
}

fn latency_bucket(duration: Duration) -> usize {
    const BOUNDS: [Duration; 7] = [
        Duration::from_millis(1),
        Duration::from_millis(10),
        Duration::from_millis(50),
        Duration::from_millis(100),
        Duration::from_millis(500),
        Duration::from_secs(1),
        Duration::from_secs(5),
    ];
    BOUNDS
        .iter()
        .position(|bound| duration < *bound)
        .unwrap_or(BOUNDS.len())
}

fn buckets_json(buckets: [u64; 8]) -> Value {
    json!({
        "lt1Ms": buckets[0],
        "lt10Ms": buckets[1],
        "lt50Ms": buckets[2],
        "lt100Ms": buckets[3],
        "lt500Ms": buckets[4],
        "lt1S": buckets[5],
        "lt5S": buckets[6],
        "gte5S": buckets[7],
    })
}

fn with_common_fields(
    mut metrics: Value,
    event: &'static str,
    route_generation: u64,
    elapsed: Duration,
) -> Value {
    if let Some(object) = metrics.as_object_mut() {
        object.insert("event".to_string(), Value::String(event.to_string()));
        object.insert("routeGeneration".to_string(), Value::from(route_generation));
        object.insert("windowMs".to_string(), Value::from(duration_ms(elapsed)));
    }
    metrics
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::mem::size_of_val;
    use std::time::{Duration, Instant};

    use super::{AcpPipelineDiagnostics, PipelineUpdateKind};

    #[test]
    fn production_mode_emits_only_terminal_summary() {
        let started_at = Instant::now();
        let mut diagnostics = AcpPipelineDiagnostics::new(started_at, false, 7);
        diagnostics.observe_frame(
            128,
            1,
            Duration::from_millis(12),
            PipelineUpdateKind::AgentText,
        );

        assert!(
            diagnostics
                .take_detailed_window(started_at + Duration::from_secs(5))
                .is_none()
        );
        let summary = diagnostics.finish(
            started_at + Duration::from_secs(6),
            "ok",
            Default::default(),
        );
        assert_eq!(summary["event"], "acp_pipeline_summary");
        assert_eq!(summary["receivedFrames"], 1);
        assert_eq!(summary["routeGeneration"], 7);
    }

    #[test]
    fn detailed_mode_emits_and_resets_fixed_five_second_window() {
        let started_at = Instant::now();
        let mut diagnostics = AcpPipelineDiagnostics::new(started_at, true, 11);
        diagnostics.observe_frame(
            64,
            1,
            Duration::from_millis(3),
            PipelineUpdateKind::ToolCallUpdate,
        );

        assert!(
            diagnostics
                .take_detailed_window(started_at + Duration::from_millis(4_999))
                .is_none()
        );
        let window = diagnostics
            .take_detailed_window(started_at + Duration::from_secs(5))
            .expect("five-second detailed window");
        assert_eq!(window["event"], "acp_pipeline_window");
        assert_eq!(window["receivedFrames"], 1);
        assert_eq!(window["updateKindCounts"]["toolCallUpdate"], 1);

        let next = diagnostics
            .take_detailed_window(started_at + Duration::from_secs(10))
            .expect("next empty window remains periodic");
        assert_eq!(next["receivedFrames"], 0);
    }

    #[test]
    fn frame_volume_does_not_grow_aggregator_memory() {
        let started_at = Instant::now();
        let mut diagnostics = AcpPipelineDiagnostics::new(started_at, true, 1);
        let before = size_of_val(&diagnostics);
        for _ in 0..10_000 {
            diagnostics.observe_frame(32, 1, Duration::from_millis(25), PipelineUpdateKind::Other);
        }
        assert_eq!(size_of_val(&diagnostics), before);

        let summary = diagnostics.finish(
            started_at + Duration::from_secs(1),
            "ok",
            Default::default(),
        );
        assert_eq!(summary["receivedFrames"], 10_000);
        assert_eq!(
            summary["queueWaitBuckets"]["lt50Ms"],
            serde_json::Value::from(10_000)
        );
    }
}
