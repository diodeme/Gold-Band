use crate::app::{LogSource, TaskSummary};
use crate::config::ConsoleThemeName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Welcome,
    TaskPicker,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Welcome,
    TaskPicker,
    Dag,
    Detail,
    Input,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    TooSmall,
    Compact,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u16,
    pub height: u16,
}

pub const MIN_VIEWPORT_WIDTH: u16 = 80;
pub const MIN_VIEWPORT_HEIGHT: u16 = 24;

impl Viewport {
    pub fn layout_mode(self) -> LayoutMode {
        if self.width < MIN_VIEWPORT_WIDTH || self.height < MIN_VIEWPORT_HEIGHT {
            LayoutMode::TooSmall
        } else if self.width < 120 || self.height < 30 {
            LayoutMode::Compact
        } else {
            LayoutMode::Full
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeAction {
    AddTask,
    SelectTask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceSelection {
    TaskOverview,
    Node { node_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailSelection {
    RetryAction,
    Attempt { attempt_id: String },
    Artifact { attempt_id: String, name: String },
    Attachment { attempt_id: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailLevel {
    NodeHome,
    AttemptItems { attempt_id: String, follow_live: bool },
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CommandViewKind {
    Help,
    Log,
    Config,
    ContinueResult,
    RuntimeCommand,
    Notice,
}

impl CommandViewKind {
    pub fn title(self) -> &'static str {
        match self {
            CommandViewKind::Help => "Help Overlay",
            CommandViewKind::Log => "Runtime Log Overlay",
            CommandViewKind::Config => "Runtime Config Overlay",
            CommandViewKind::ContinueResult => "Continue Result Overlay",
            CommandViewKind::RuntimeCommand => "Command Result Overlay",
            CommandViewKind::Notice => "Notice Overlay",
        }
    }

    pub fn body_title(self) -> &'static str {
        match self {
            CommandViewKind::Help => "Help",
            CommandViewKind::Log => "Runtime Log",
            CommandViewKind::Config => "Runtime Config",
            CommandViewKind::ContinueResult => "Continue Result",
            CommandViewKind::RuntimeCommand => "Command Result",
            CommandViewKind::Notice => "Notice",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayState {
    pub kind: CommandViewKind,
    pub body: String,
    pub scroll: u16,
    pub return_focus: FocusPane,
}

#[derive(Debug, Clone)]
pub struct WorkspaceState {
    pub task_id: String,
    pub task_summary: TaskSummary,
    pub active_run_id: Option<String>,
    pub selected_round_id: Option<String>,
    pub run_progress_summary: Option<String>,
    pub run_events_tail: Option<String>,
    pub selection: WorkspaceSelection,
    pub dag_positions: Vec<Vec<String>>,
    pub dag_column: usize,
    pub dag_row: usize,
    pub detail_level: DetailLevel,
    pub detail_items: Vec<DetailSelection>,
    pub detail_index: usize,
    pub detail_scroll: u16,
    pub log_source: LogSource,
    pub log_scroll: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundTaskState {
    pub task_id: String,
    pub kind: &'static str,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConsoleState {
    pub screen: Screen,
    pub focus: FocusPane,
    pub input: String,
    pub history: Vec<String>,
    pub message: Option<String>,
    pub auto_refresh_enabled: bool,
    pub last_refresh_label: Option<String>,
    pub viewport: Viewport,
    pub layout_mode: LayoutMode,
    pub welcome_action: WelcomeAction,
    pub console_theme: ConsoleThemeName,
    pub task_list: Vec<TaskSummary>,
    pub task_index: usize,
    pub workspace: Option<WorkspaceState>,
    pub overlay: Option<OverlayState>,
    pub command_suggestions: Vec<String>,
    pub background_task: Option<BackgroundTaskState>,
}

impl Default for ConsoleState {
    fn default() -> Self {
        Self {
            screen: Screen::Welcome,
            focus: FocusPane::Welcome,
            input: String::new(),
            history: Vec::new(),
            message: None,
            auto_refresh_enabled: true,
            last_refresh_label: None,
            viewport: Viewport { width: 120, height: 40 },
            layout_mode: LayoutMode::Full,
            welcome_action: WelcomeAction::SelectTask,
            console_theme: ConsoleThemeName::GoldBand,
            task_list: Vec::new(),
            task_index: 0,
            workspace: None,
            overlay: None,
            command_suggestions: Vec::new(),
            background_task: None,
        }
    }
}
