use crate::app::App;
use crate::app_types::{COMMANDS, Mode};
use crate::message::FrontendMessage;
use crate::settings::{SettingsLoadResult, SettingsState};

/// Execute a `:command` string entered by the user.
/// Returns an optional FrontendMessage to send to di-core.
pub fn execute_command(app: &mut App, cmd: &str) -> Option<FrontendMessage> {
    match cmd {
        "q" | "quit" => {
            app.should_quit = true;
            None
        }
        "settings" => {
            app.settings = Some(SettingsState::new_empty());
            app.mode = Mode::Settings;
            if let Some(tx) = &app.event_tx {
                let tx = tx.clone();
                tokio::task::spawn_blocking(move || {
                    let mut gw = crate::settings::GatewayConnection::new();
                    let gateway_ok = gw.ensure_connected().is_ok();
                    let providers = if gateway_ok {
                        crate::settings::query_list_providers(&mut gw).unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let all = crate::settings::load_all_settings();
                    let rs = all.roles.get("act").cloned().unwrap_or_default();
                    let (fields, models, provider_info, gateway_error) =
                        crate::settings::build_role_fields(&rs, &providers, &mut gw, gateway_ok);
                    let _ = tx.send(crate::AppEvent::SettingsLoaded(
                        SettingsLoadResult::Initial {
                            providers,
                            fields,
                            model_entries: models,
                            provider_info,
                            gateway_available: gateway_ok,
                            gateway_error,
                        }
                    ));
                });
            }
            None
        }
        "interrupt" => {
            if let Some(agent) = app.active_agent() {
                let id = agent.id;
                Some(FrontendMessage::Interrupt { agent_id: id })
            } else {
                None
            }
        }
        "close" => {
            app.close_active_tab()
        }
        "plan" | "act" => {
            let mode = cmd; // "plan" or "act"
            let agent_id = app.active_agent().map(|a| a.id);
            if let Some(id) = agent_id {
                app.status_message = Some(format!("Switched to {} mode", mode));
                if let Some(agent) = app.active_agent_mut() {
                    agent.mode = mode.to_string();
                }
                Some(FrontendMessage::SetMode { agent_id: id, mode: mode.to_string() })
            } else {
                app.status_message = Some("No active agent".to_string());
                None
            }
        }
        _ if cmd.starts_with("new ") => {
            let task = cmd[4..].trim().to_string();
            if task.is_empty() {
                app.status_message = Some("Task cannot be empty".to_string());
                None
            } else if task.len() > 10_000 {
                app.status_message = Some("Task too long (max 10,000 chars)".to_string());
                None
            } else if task.chars().any(|c| c.is_control()) {
                app.status_message = Some("Task contains control characters".to_string());
                None
            } else {
                queue_provider_config(app);
                Some(FrontendMessage::SpawnAgent { task })
            }
        }
        _ => {
            app.status_message = Some(format!("Unknown command: :{}", cmd));
            None
        }
    }
}

/// Queue SetProviderConfig messages for all configured roles into pending_messages.
fn queue_provider_config(app: &mut App) {
    let all = crate::settings::load_all_settings();
    for role in crate::settings::ROLES {
        if let Some(rs) = all.roles.get(*role) {
            if !rs.provider.is_empty() && !rs.model.is_empty() {
                let params: std::collections::HashMap<String, serde_json::Value> =
                    rs.provider_params.iter().map(|(k, v)| {
                        (k.clone(), crate::settings::string_to_json_value(v))
                    }).collect();
                app.pending_messages.push(FrontendMessage::SetProviderConfig {
                    role: role.to_string(),
                    provider: rs.provider.clone(),
                    model: rs.model.clone(),
                    api_key: if rs.api_key.is_empty() { None } else { Some(rs.api_key.clone()) },
                    base_url: if rs.base_url.is_empty() { None } else { Some(rs.base_url.clone()) },
                    params,
                });
            }
        }
    }
}

/// Rebuild the command palette from COMMANDS filtered by the current buffer.
pub fn refresh_palette(app: &mut App) {
    let prefix = app.command_buffer.trim();
    app.command_palette = COMMANDS.iter()
        .filter(|cmd| {
            if prefix.is_empty() { return true; }
            cmd.name.starts_with(prefix) || cmd.prefix == prefix
        })
        .cloned()
        .collect();
    app.palette_cursor = 0;
}
