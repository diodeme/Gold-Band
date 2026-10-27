//! Only the Card JSON 2.0 components emitted by this connector. Dynamic
//! answers remain in the domain; the outbound protocol is typed throughout.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Card {
    schema: Schema,
    config: Config,
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<Header>,
    body: Body,
}

#[derive(Serialize)]
enum Schema {
    #[serde(rename = "2.0")]
    V2,
}

#[derive(Serialize)]
struct Config {
    update_multi: bool,
}

#[derive(Serialize)]
struct Body {
    elements: Vec<Element>,
}

#[derive(Serialize)]
pub(super) struct Header {
    pub title: Text,
    pub template: HeaderTemplate,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HeaderTemplate {
    Blue,
    Grey,
}

impl Card {
    pub fn new(elements: Vec<Element>, header: Option<Header>) -> Self {
        Self {
            schema: Schema::V2,
            config: Config { update_multi: true },
            header,
            body: Body { elements },
        }
    }

    pub fn content(&self) -> String {
        serde_json::to_string(self).expect("typed Feishu card is JSON serializable")
    }
}

#[derive(Serialize)]
pub(super) struct Reply {
    toast: Toast,
    #[serde(skip_serializing_if = "Option::is_none")]
    card: Option<RawCard>,
}

#[derive(Serialize)]
struct Toast {
    #[serde(rename = "type")]
    kind: ToastType,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ToastType {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Serialize)]
#[serde(tag = "type", rename = "raw")]
struct RawCard {
    data: Card,
}

impl Reply {
    pub fn new(kind: ToastType, content: &str, card: Option<Card>) -> Self {
        Self {
            toast: Toast {
                kind,
                content: content.to_owned(),
            },
            card: card.map(|data| RawCard { data }),
        }
    }

    pub fn into_value(self) -> serde_json::Value {
        serde_json::to_value(self).expect("typed Feishu reply is JSON serializable")
    }
}

#[derive(Serialize)]
#[serde(tag = "tag", rename_all = "snake_case")]
pub(super) enum Text {
    PlainText { content: String },
}

impl Text {
    pub fn plain(content: &str) -> Self {
        Self::PlainText {
            content: content.to_owned(),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "tag", rename_all = "snake_case")]
pub(super) enum Element {
    Markdown {
        content: String,
    },
    Form {
        name: String,
        elements: Vec<Element>,
    },
    Button(Button),
    SelectStatic(SingleSelect),
    MultiSelectStatic(MultiSelect),
    Input(Input),
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ButtonType {
    Primary,
    Default,
    Danger,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FormAction {
    Submit,
}

#[derive(Serialize)]
pub(super) struct Button {
    pub text: Text,
    #[serde(rename = "type")]
    pub button_type: ButtonType,
    pub behaviors: Vec<Behavior>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_action_type: Option<FormAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Behavior {
    Callback { value: CallbackValue },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CallbackValue {
    pub delivery_id: String,
    pub action_token: String,
    #[serde(flatten)]
    pub intent: CallbackIntent,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum CallbackIntent {
    Decline {
        decline: bool,
    },
    Index {
        #[serde(rename = "actionIndex")]
        action_index: usize,
    },
    Form {
        form: bool,
    },
    PermissionSelect {
        #[serde(rename = "permissionSelect")]
        permission_select: bool,
    },
}

#[derive(Serialize)]
pub(super) struct SelectOption {
    pub text: Text,
    pub value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Width {
    Fill,
}

#[derive(Serialize)]
pub(super) struct Select {
    pub name: String,
    pub required: bool,
    pub width: Width,
    pub placeholder: Text,
    pub options: Vec<SelectOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

#[derive(Serialize)]
pub(super) struct SingleSelect {
    #[serde(flatten)]
    pub select: Select,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_index: Option<usize>,
}

#[derive(Serialize)]
pub(super) struct MultiSelect {
    #[serde(flatten)]
    pub select: Select,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_values: Option<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum InputType {
    MultilineText,
}

#[derive(Serialize)]
pub(super) struct Input {
    pub name: String,
    pub required: bool,
    pub max_length: u32,
    pub width: Width,
    pub placeholder: Text,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_type: Option<InputType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_resize: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

/// A transient projection from the committed outcome through published options.
pub(super) enum ControlValue {
    Single(Option<String>),
    Multi(Vec<String>),
    Text(String),
}

impl Element {
    pub fn disable(&mut self) {
        match self {
            Self::Form { elements, .. } => elements.iter_mut().for_each(Self::disable),
            Self::Button(button) => button.disabled = Some(true),
            Self::SelectStatic(control) => control.select.disabled = Some(true),
            Self::MultiSelectStatic(control) => control.select.disabled = Some(true),
            Self::Input(control) => control.disabled = Some(true),
            Self::Markdown { .. } => {}
        }
    }

    pub fn restore(&mut self, values: &BTreeMap<String, ControlValue>) {
        match self {
            Self::Form { elements, .. } => {
                elements.iter_mut().for_each(|child| child.restore(values))
            }
            Self::SelectStatic(control) => {
                if let Some(ControlValue::Single(value)) = values.get(&control.select.name) {
                    // Card JSON 2.0 uses a 1-based index; zero leaves it empty.
                    control.initial_index = Some(
                        control
                            .select
                            .options
                            .iter()
                            .position(|option| Some(&option.value) == value.as_ref())
                            .map_or(0, |index| index + 1),
                    );
                }
            }
            Self::MultiSelectStatic(control) => {
                if let Some(ControlValue::Multi(value)) = values.get(&control.select.name) {
                    control.selected_values = Some(value.clone());
                }
            }
            Self::Input(control) => {
                if let Some(ControlValue::Text(value)) = values.get(&control.name) {
                    control.default_value = Some(value.clone());
                }
            }
            Self::Markdown { .. } | Self::Button(_) => {}
        }
    }
}
