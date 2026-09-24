use std::collections::BTreeMap;

use crate::config::{ConversationAutoConfig, ConversationDynamicControl};
use crate::dsl::{
    AiDynamicAgentStrategy, AiDynamicNode, AllowedWorkflowRefDsl, DynamicAgentRef,
    DynamicControlDsl, END_NODE, EdgeDsl, EdgeOutcome, NodeDsl, WorkflowDsl,
};

pub fn compile_auto_workflow(config: Option<&ConversationAutoConfig>) -> WorkflowDsl {
    let agent_type = config
        .map(|config| config.agent_type.as_str())
        .unwrap_or("");
    let model_id = trimmed_opt(config.and_then(|config| config.model_id.as_deref()));
    let bootstrap_model_id =
        trimmed_opt(config.and_then(|config| config.bootstrap_model_id.as_deref()));
    let acceptance_model_id =
        trimmed_opt(config.and_then(|config| config.acceptance_model_id.as_deref()));
    let permission_mode = trimmed_opt(config.and_then(|config| config.permission_mode.as_deref()));
    let auto_accept = config.is_some_and(|config| config.auto_accept);
    let global_goal = trimmed_opt(config.and_then(|config| config.global_goal.as_deref()));
    let agent_strategy_mode = config
        .and_then(|config| config.agent_strategy.as_deref())
        .unwrap_or("fixed");

    let agent_strategy = if agent_strategy_mode == "dynamic" {
        let bootstrap_provider = config
            .and_then(|config| config.bootstrap_agent_type.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(agent_type)
            .to_string();
        let available_agents = config
            .and_then(|config| config.available_agents.as_ref())
            .map(|agents| {
                agents
                    .iter()
                    .filter_map(|agent| {
                        let provider = agent.provider.trim();
                        if provider.is_empty() {
                            return None;
                        }
                        Some(DynamicAgentRef {
                            provider: provider.to_string(),
                            model: trimmed_opt(agent.model.as_deref()),
                            permission_mode: trimmed_opt(agent.permission_mode.as_deref()),
                            auto_accept: agent.auto_accept,
                            config_options: agent.config_options.clone(),
                            model_bound_overrides: agent.model_bound_overrides.clone(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|agents| !agents.is_empty())
            .unwrap_or_else(|| {
                vec![DynamicAgentRef {
                    provider: bootstrap_provider.clone(),
                    model: model_id.clone(),
                    permission_mode: None,
                    auto_accept: false,
                    config_options: BTreeMap::new(),
                    model_bound_overrides: BTreeMap::new(),
                }]
            });
        AiDynamicAgentStrategy::Dynamic {
            bootstrap_provider,
            bootstrap_model: bootstrap_model_id,
            permission_mode: permission_mode.clone(),
            auto_accept,
            bootstrap_config_options: config
                .map(|config| config.bootstrap_config_options.clone())
                .unwrap_or_default(),
            bootstrap_model_bound_overrides: config
                .map(|config| config.bootstrap_model_bound_overrides.clone())
                .unwrap_or_default(),
            acceptance_model: acceptance_model_id,
            acceptance_config_options: config
                .map(|config| config.acceptance_config_options.clone())
                .unwrap_or_default(),
            acceptance_model_bound_overrides: config
                .map(|config| config.acceptance_model_bound_overrides.clone())
                .unwrap_or_default(),
            routing_prompt: config
                .and_then(|config| config.routing_prompt.as_deref())
                .unwrap_or("")
                .trim()
                .to_string(),
            available_agents,
        }
    } else {
        AiDynamicAgentStrategy::Fixed {
            provider: agent_type.to_string(),
            model: model_id,
            permission_mode,
            auto_accept,
        }
    };

    WorkflowDsl {
        version: "0.1".to_string(),
        id: "auto-workflow".to_string(),
        entry: "ai-dynamic".to_string(),
        control: Default::default(),
        nodes: vec![NodeDsl::AiDynamic(AiDynamicNode {
            id: "ai-dynamic".to_string(),
            agent_strategy,
            config_options: config
                .map(|config| config.config_options.clone())
                .unwrap_or_default(),
            model_bound_overrides: config
                .map(|config| config.model_bound_overrides.clone())
                .unwrap_or_default(),
            allowed_profiles: config
                .and_then(|config| config.allowed_profiles.clone())
                .unwrap_or_default(),
            global_goal,
            control: control_from(config.and_then(|config| config.control.as_ref())),
            allowed_workflows: config
                .and_then(|config| config.allowed_workflows.as_ref())
                .map(|workflows| {
                    workflows
                        .iter()
                        .filter_map(|workflow| {
                            let workflow_id = workflow.workflow_id.trim();
                            (!workflow_id.is_empty()).then(|| AllowedWorkflowRefDsl {
                                workflow_id: workflow_id.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })],
        edges: vec![EdgeDsl {
            from: "ai-dynamic".to_string(),
            to: END_NODE.to_string(),
            on: EdgeOutcome::Success,
            session: None,
            new_round_entry: None,
        }],
    }
}

fn control_from(control: Option<&ConversationDynamicControl>) -> DynamicControlDsl {
    control
        .map(|control| DynamicControlDsl {
            max_dynamic_nodes: control.max_dynamic_nodes,
            max_fanout: control.max_fanout,
            max_depth: control.max_depth,
            max_parallel: control.max_parallel,
            max_group_depth: control.max_group_depth,
            max_workflow_invocations: control.max_workflow_invocations,
            allow_nested_dynamic: control.allow_nested_dynamic,
        })
        .unwrap_or_default()
}

fn trimmed_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
