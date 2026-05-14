use chrono::Utc;

use crate::agent::{AgentState, AgentStatus, PendingInput};
use crate::app::App;
use crate::message::{CoreEvent, FrontendMessage};

/// Process a CoreEvent from di-core and update the App state accordingly.
/// Called from the main event loop for every core event received.
pub fn handle_core_event(app: &mut App, event: CoreEvent) {
    let agent_id = match &event {
        CoreEvent::TaskInitialized { agent_id, .. } => *agent_id,
        CoreEvent::ThoughtDelta { agent_id, .. } => *agent_id,
        CoreEvent::ThoughtFinished { agent_id } => *agent_id,
        CoreEvent::ToolCallStarted { agent_id, .. } => *agent_id,
        CoreEvent::ToolCallFinished { agent_id, .. } => *agent_id,
        CoreEvent::ApprovalNeeded { agent_id, .. } => *agent_id,
        CoreEvent::FollowupQuestion { agent_id, .. } => *agent_id,
        CoreEvent::MetricsUpdate { agent_id, .. } => *agent_id,
        CoreEvent::TaskFinished { agent_id, .. } => *agent_id,
        CoreEvent::TaskPresented { agent_id, .. } => *agent_id,
        CoreEvent::ContextCompacted { agent_id, .. } => *agent_id,
        CoreEvent::BackgroundCommandStarted { agent_id, .. } => *agent_id,
        CoreEvent::BackgroundCommandFinished { agent_id, .. } => *agent_id,
        CoreEvent::ObserverSignal { agent_id, .. } => *agent_id,
        CoreEvent::FrontendTimeout { agent_id, .. } => *agent_id,
    };

    match event {
        CoreEvent::TaskInitialized { agent_id, .. } => {
            if app.agents.iter().any(|a| a.id == agent_id) {
                crate::app::log_event(&format!("Duplicate agent_id ignored: {}", agent_id));
                return;
            }
            let idx = app.agents.len() + 1;
            let agent = AgentState::new(agent_id, format!("Agent-{}", idx));
            app.agents.push(agent);
            app.active_tab = app.active_tab.min(app.agents.len() - 1);
            app.status_message = Some(format!("Agent-{} initialized", idx));
        }
        CoreEvent::ThoughtDelta { text, thinking, .. } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                let is_thinking = thinking;
                let was_thinking = agent.log.streaming_is_thinking();
                let has_streaming = agent.log.streaming().is_some();

                if !has_streaming {
                    agent.log.set_streaming(
                        if is_thinking { format!("{} {}", crate::summarize::THINKING_PREFIX, text) } else { text.clone() },
                        is_thinking,
                    );
                    agent.status = AgentStatus::Running;
                } else if was_thinking != is_thinking {
                    agent.log.finalize_streaming();
                    agent.log.set_streaming(
                        if is_thinking { format!("{} {}", crate::summarize::THINKING_PREFIX, text) } else { text.clone() },
                        is_thinking,
                    );
                } else {
                    agent.log.append_streaming(&text);
                }
                agent.last_activity = Utc::now();
            }
        }
        CoreEvent::ThoughtFinished { .. } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.finalize_streaming();
                agent.last_activity = Utc::now();
            }
        }
        CoreEvent::ToolCallStarted { call_id, tool, args, .. } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.finalize_streaming();
                let summary = crate::summarize::summarize_tool_args(&tool, &args);
                agent.log.push_tool_call(call_id, tool, summary);
                agent.status = AgentStatus::Running;
                agent.last_activity = Utc::now();
            }
        }
        CoreEvent::ToolCallFinished { call_id, result, .. } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.finalize_streaming();
                let status = result
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("done");
                let msg = if status == "denied" {
                    format!("Denied: {}", result.get("message").and_then(|v| v.as_str()).unwrap_or(""))
                } else if status == "compacted" {
                    "Context compacted".to_string()
                } else if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
                    format!("Error: {}", err)
                } else {
                    crate::summarize::format_result_summary(&result)
                };
                agent.log.set_tool_result(&call_id, msg);
                agent.last_activity = Utc::now();
            }
        }
        CoreEvent::ApprovalNeeded {
            tool, args, description, ..
        } => {
            if app.auto_approve {
                app.input_queue.retain(|(id, _)| *id != agent_id);
                if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                    agent.pending_input = None;
                }
                app.pending_messages.push(FrontendMessage::ApprovalResponse {
                    agent_id,
                    approved: true,
                });
            } else {
                app.input_queue.retain(|(id, _)| *id != agent_id);
                let pending = PendingInput::Approval {
                    tool,
                    args,
                    description,
                };
                if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                    agent.log.finalize_streaming();
                    agent.pending_input = Some(pending.clone());
                    agent.status = AgentStatus::Waiting;
                    agent.last_activity = Utc::now();
                }
                app.input_queue.push((agent_id, pending));
            }
        }
        CoreEvent::FollowupQuestion {
            question, options, ..
        } => {
            app.input_queue.retain(|(id, _)| *id != agent_id);
            let pending = PendingInput::Followup {
                question: question.clone(),
                options: options.clone(),
            };
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.finalize_streaming();
                agent.pending_input = Some(pending.clone());
                agent.status = AgentStatus::Waiting;
                agent.log.push_assistant(question);
                agent.last_activity = Utc::now();
            }
            app.input_queue.push((agent_id, pending));
        }
        CoreEvent::MetricsUpdate {
            sqs,
            token_usage,
            latency_ms,
            ..
        } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.metrics = Some(crate::agent::Metrics {
                    sqs,
                    token_usage,
                    latency_ms,
                });
            }
        }
        CoreEvent::TaskFinished {
            success, message, ..
        } => {
            app.input_queue.retain(|(id, _)| *id != agent_id);
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                if matches!(agent.status, AgentStatus::Finished | AgentStatus::Error) {
                    crate::app::log_event(&format!("Duplicate TaskFinished for {} ignored", agent_id));
                    return;
                }
                agent.status = if success {
                    AgentStatus::Finished
                } else {
                    AgentStatus::Error
                };
                agent.log.clear_streaming();
                agent.pending_input = None;
                let msg = if success {
                    format!("Task complete: {}", message)
                } else {
                    format!("Task ended: {}", message)
                };
                agent.log.push_finish(msg, success);
                agent.last_activity = Utc::now();
            }
        }
        CoreEvent::TaskPresented { message, .. } => {
            app.input_queue.retain(|(id, _)| *id != agent_id);
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.finalize_streaming();
                agent.status = AgentStatus::Finished;
                agent.pending_input = None;
                agent.log.push_system(format!("Result: {}", message));
                agent.last_activity = Utc::now();
            }
        }
        CoreEvent::ContextCompacted {
            remaining_tokens, ..
        } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.finalize_streaming();
                agent.log.push_system(format!("Context compacted ({} tokens remaining)", remaining_tokens));
            }
        }
        CoreEvent::BackgroundCommandStarted {
            command_id,
            command,
            ..
        } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.push_system(format!("Background: {} ({})", command, command_id));
            }
        }
        CoreEvent::BackgroundCommandFinished {
            command_id,
            exit_code,
            ..
        } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.push_system(format!(
                    "Background {} done (exit: {})",
                    command_id,
                    exit_code.map(|c| c.to_string()).unwrap_or("?".to_string())
                ));
            }
        }
        CoreEvent::ObserverSignal { message, .. } => {
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.log.push_system(format!("[observer] {}", message));
            }
        }
        CoreEvent::FrontendTimeout { tool, question, .. } => {
            app.input_queue.retain(|(id, _)| *id != agent_id);
            if let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.pending_input = None;
                if agent.status == AgentStatus::Waiting {
                    agent.status = AgentStatus::Running;
                }
                let detail = tool.as_deref().unwrap_or_else(|| question.as_deref().unwrap_or("unknown"));
                agent.log.push_system(format!("Timed out waiting for response: {}", detail));
            }
        }
    }
}
