use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Agent roles — extensible list
// ---------------------------------------------------------------------------

pub const ROLES: &[&str] = &["act", "plan", "distiller", "observer"];

pub fn role_label(role: &str) -> &str {
    match role {
        "act" => "Act",
        "plan" => "Plan",
        "distiller" => "Distiller",
        "observer" => "Observer",
        _ => role,
    }
}

pub fn string_to_json_value(val: &str) -> serde_json::Value {
    if val == "on" || val == "true" {
        serde_json::Value::Bool(true)
    } else if val == "off" || val == "false" {
        serde_json::Value::Bool(false)
    } else if let Ok(n) = val.parse::<f64>() {
        serde_json::json!(n)
    } else {
        serde_json::Value::String(val.to_string())
    }
}

// ---------------------------------------------------------------------------
// Gateway response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct GatewayResponse {
    #[allow(dead_code)]
    id: i64,
    pub(crate) status: i64,
    pub(crate) body: Option<serde_json::Value>,
    pub(crate) error: Option<GatewayError>,
}

#[derive(Deserialize)]
pub(crate) struct GatewayError {
    pub(crate) message: String,
}

// ---------------------------------------------------------------------------
// Provider metadata and model entries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderMeta {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub context_window: Option<i64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub supports_thinking: Option<bool>,
}

// ---------------------------------------------------------------------------
// Provider info — discoverable parameters from gateway
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInfo {
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub default_model: String,
    #[serde(default)]
    pub settings: Vec<ProviderSetting>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSetting {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub options: Vec<SelectOption>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectOption {
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// Settings field types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldKind {
    Selector,
    Text,
    Secret,
}

#[derive(Debug, Clone)]
pub enum SettingsField {
    Selector {
        label: String,
        options: Vec<String>,
        labels: Vec<String>,
        index: usize,
    },
    Text {
        label: String,
        value: String,
    },
    Secret {
        label: String,
        value: String,
    },
}

impl SettingsField {
    pub fn kind(&self) -> FieldKind {
        match self {
            SettingsField::Selector { .. } => FieldKind::Selector,
            SettingsField::Text { .. } => FieldKind::Text,
            SettingsField::Secret { .. } => FieldKind::Secret,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            SettingsField::Selector { label, .. } => label,
            SettingsField::Text { label, .. } => label,
            SettingsField::Secret { label, .. } => label,
        }
    }

    pub fn display_value(&self) -> String {
        match self {
            SettingsField::Selector { options, labels, index, .. } => {
                if options.is_empty() {
                    "(none)".into()
                } else {
                    let i = (*index).min(options.len() - 1);
                    labels[i].clone()
                }
            }
            SettingsField::Text { value, .. } => {
                if value.is_empty() { String::new() } else { value.clone() }
            }
            SettingsField::Secret { value, .. } => {
                if value.is_empty() { String::new() } else { "*".repeat(value.len().min(30)) }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-role provider settings
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RoleSettings {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub provider_params: HashMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RoleBehaviorSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_turns: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_critic_frequency: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_verbose: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_token_threshold: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_buffer_activation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_block_after: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_reflection_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observer_reflection_token_threshold: Option<u32>,
}

impl RoleBehaviorSettings {
    #[allow(dead_code)]
    pub fn defaults_for(role: &str) -> Self {
        match role {
            "observer" => Self {
                enabled: Some(false),
                observer_turns: Some(2),
                observer_critic_frequency: Some(6),
                observer_verbose: Some(false),
                observer_token_threshold: Some(15000),
                observer_buffer_activation: Some(0.8),
                observer_block_after: Some(0.7),
                observer_reflection_enabled: Some(true),
                observer_reflection_token_threshold: Some(10000),
            },
            "distiller" => Self {
                enabled: Some(false),
                ..Default::default()
            },
            _ => Self::default(),
        }
    }

    #[allow(dead_code)]
    pub fn has_settings(&self, role: &str) -> bool {
        matches!(role, "observer" | "distiller")
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AllSettings {
    #[serde(flatten)]
    pub roles: HashMap<String, RoleSettings>,
    #[serde(default)]
    pub behaviors: HashMap<String, RoleBehaviorSettings>,
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_theme() -> String {
    "copper-cobalt-dimmed".to_string()
}

impl Default for AllSettings {
    fn default() -> Self {
        Self {
            roles: HashMap::new(),
            behaviors: HashMap::new(),
            theme: default_theme(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsPanel {
    Provider,
    Role,
    Theme,
}

// ---------------------------------------------------------------------------
// Async operation descriptors
// ---------------------------------------------------------------------------

/// Describes an async gateway operation the app should spawn.
#[derive(Debug)]
#[allow(dead_code)]
pub enum PendingAsyncOp {
    ProviderChanged { seq: u64, rs: RoleSettings, providers: Vec<ProviderMeta>, gateway_available: bool },
    RoleSwitched { seq: u64, rs: RoleSettings, providers: Vec<ProviderMeta>, gateway_available: bool },
    Save { all_settings: AllSettings },
}

/// Result of an async gateway operation, sent back through the event loop.
#[derive(Debug)]
pub enum SettingsLoadResult {
    Initial {
        providers: Vec<ProviderMeta>,
        fields: Vec<SettingsField>,
        model_entries: Vec<ModelEntry>,
        provider_info: Option<ProviderInfo>,
        gateway_available: bool,
        gateway_error: Option<String>,
    },
    ProviderChanged {
        seq: u64,
        fields: Vec<SettingsField>,
        model_entries: Vec<ModelEntry>,
        provider_info: Option<ProviderInfo>,
        gateway_error: Option<String>,
    },
    RoleSwitched {
        seq: u64,
        fields: Vec<SettingsField>,
        model_entries: Vec<ModelEntry>,
        provider_info: Option<ProviderInfo>,
        gateway_error: Option<String>,
    },
    Saved {
        error: Option<String>,
        messages: Vec<crate::message::FrontendMessage>,
    },
}

// Field offsets within cursor (cursor 0 = role tabs, 1+ = fields)
pub const F_PROVIDER: usize = 0;
pub const F_API_KEY: usize = 1;
pub const F_MODEL: usize = 2;
pub const F_BASE_URL: usize = 3;
pub const NUM_BASE_FIELDS: usize = 4;

// ---------------------------------------------------------------------------
// Field construction helpers
// ---------------------------------------------------------------------------

/// Build minimal provider fields without gateway data (used during loading/panel switch).
/// Preserves the saved model name so it isn't lost when gateway is unavailable.
pub fn build_minimal_base_fields(rs: &RoleSettings) -> Vec<SettingsField> {
    let (model_options, model_labels) = if rs.model.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (vec![rs.model.clone()], vec![rs.model.clone()])
    };
    let (provider_options, provider_labels) = if rs.provider.is_empty() {
        (vec![String::new()], vec!["(none)".to_string()])
    } else {
        (vec![rs.provider.clone()], vec![rs.provider.clone()])
    };
    vec![
        SettingsField::Selector {
            label: "Provider".to_string(),
            options: provider_options,
            labels: provider_labels,
            index: 0,
        },
        SettingsField::Secret {
            label: "API Key".to_string(),
            value: rs.api_key.clone(),
        },
        SettingsField::Selector {
            label: "Model".to_string(),
            options: model_options,
            labels: model_labels,
            index: 0,
        },
        SettingsField::Text {
            label: "Base URL".to_string(),
            value: rs.base_url.clone(),
        },
    ]
}

pub fn toggle_field(label: &str, value: bool) -> SettingsField {
    SettingsField::Selector {
        label: label.to_string(),
        options: vec!["off".to_string(), "on".to_string()],
        labels: vec!["Off".to_string(), "On".to_string()],
        index: if value { 1 } else { 0 },
    }
}

pub fn number_field(label: &str, value: Option<u32>) -> SettingsField {
    SettingsField::Text {
        label: label.to_string(),
        value: value.map(|v| v.to_string()).unwrap_or_default(),
    }
}

pub fn decimal_field(label: &str, value: Option<f64>) -> SettingsField {
    SettingsField::Text {
        label: label.to_string(),
        value: value.map(|v| format!("{:.2}", v)).unwrap_or_default(),
    }
}

/// Build behavior fields for a given role.
pub fn build_role_behavior_fields(role: &str, beh: &RoleBehaviorSettings) -> Vec<SettingsField> {
    match role {
        "observer" => vec![
            toggle_field("Enabled", beh.enabled.unwrap_or(false)),
            number_field("Turns", beh.observer_turns),
            number_field("Critic Frequency", beh.observer_critic_frequency),
            toggle_field("Verbose", beh.observer_verbose.unwrap_or(false)),
            number_field("Token Threshold", beh.observer_token_threshold),
            decimal_field("Buffer Activation", beh.observer_buffer_activation),
            decimal_field("Block After", beh.observer_block_after),
            toggle_field("Reflection", beh.observer_reflection_enabled.unwrap_or(true)),
            number_field("Reflection Threshold", beh.observer_reflection_token_threshold),
        ],
        "distiller" => vec![
            toggle_field("Enabled", beh.enabled.unwrap_or(false)),
        ],
        _ => Vec::new(),
    }
}

pub fn build_theme_fields(current: &str) -> Vec<SettingsField> {
    let names = crate::theme::Theme::theme_names();
    let labels: Vec<String> = crate::theme::Theme::theme_labels().iter().map(|s| s.to_string()).collect();
    let options: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    let index = names.iter().position(|&n| n == current).unwrap_or(0);
    vec![
        SettingsField::Selector {
            label: "Theme".to_string(),
            options,
            labels,
            index,
        },
    ]
}

/// Build minimal base fields when gateway queries fail.
/// Preserves the saved model name so it isn't lost when models can't be fetched.
pub fn build_minimal_fields(rs: &RoleSettings, providers: &[ProviderMeta]) -> Vec<SettingsField> {
    let (provider_index, provider_options, provider_labels) = if rs.provider.is_empty() {
        let mut options = vec![String::new()];
        let mut labels = vec!["(none)".to_string()];
        options.extend(providers.iter().map(|p| p.id.clone()));
        labels.extend(providers.iter().map(|p| format!("{} ({})", p.label, p.id)));
        (0, options, labels)
    } else {
        let idx = providers.iter().position(|p| p.id == rs.provider).unwrap_or(0);
        let labels: Vec<String> = providers.iter().map(|p| format!("{} ({})", p.label, p.id)).collect();
        let options: Vec<String> = providers.iter().map(|p| p.id.clone()).collect();
        (idx, options, labels)
    };
    let (model_options, model_labels) = if rs.model.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (vec![rs.model.clone()], vec![rs.model.clone()])
    };

    vec![
        SettingsField::Selector {
            label: "Provider".to_string(),
            options: provider_options,
            labels: provider_labels,
            index: provider_index,
        },
        SettingsField::Secret {
            label: "API Key".to_string(),
            value: rs.api_key.clone(),
        },
        SettingsField::Selector {
            label: "Model".to_string(),
            options: model_options,
            labels: model_labels,
            index: 0,
        },
        SettingsField::Text {
            label: "Base URL".to_string(),
            value: rs.base_url.clone(),
        },
    ]
}

pub fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => if *b { "on".to_string() } else { "off".to_string() },
        _ => String::new(),
    }
}

fn field_to_param_value(field: &SettingsField, ps: &ProviderSetting) -> String {
    match field {
        SettingsField::Selector { options, index, .. } => {
            options.get(*index).cloned().unwrap_or_default()
        }
        SettingsField::Text { value, .. } | SettingsField::Secret { value, .. } => {
            if ps.setting_type == "slider" || ps.setting_type == "number" {
                if value.is_empty() { return String::new(); }
                if let Ok(n) = value.parse::<f64>() {
                    if let Some(min) = ps.min { if n < min { return min.to_string(); } }
                    if let Some(max) = ps.max { if n > max { return max.to_string(); } }
                }
            }
            value.clone()
        }
    }
}

pub fn provider_setting_to_field(ps: &ProviderSetting, current_value: Option<&String>) -> SettingsField {
    let val = current_value.cloned().unwrap_or_else(|| {
        ps.default.as_ref().map(value_to_string).unwrap_or_default()
    });

    match ps.setting_type.as_str() {
        "toggle" => {
            let options = vec!["off".to_string(), "on".to_string()];
            let labels = vec!["Off".to_string(), "On".to_string()];
            let index = if val == "on" || val == "true" || val == "1" { 1 } else { 0 };
            SettingsField::Selector {
                label: ps.label.clone(),
                options,
                labels,
                index,
            }
        }
        "select" => {
            let mut options: Vec<String> = ps.options.iter().map(|o| o.value.clone()).collect();
            let mut labels: Vec<String> = ps.options.iter()
                .map(|o| o.label.clone().unwrap_or_else(|| o.value.clone()))
                .collect();
            let index = options.iter().position(|o| o == &val).unwrap_or_else(|| {
                options.push(val.clone());
                labels.push(format!("{} (deprecated)", val));
                options.len() - 1
            });
            SettingsField::Selector {
                label: ps.label.clone(),
                options,
                labels,
                index,
            }
        }
        "slider" | "number" | "text" => {
            SettingsField::Text {
                label: ps.label.clone(),
                value: val,
            }
        }
        _ => {
            SettingsField::Text {
                label: ps.label.clone(),
                value: val,
            }
        }
    }
}

pub fn gather_role_settings(fields: &[SettingsField], provider_settings: &[ProviderSetting]) -> RoleSettings {
    if fields.len() < NUM_BASE_FIELDS {
        return RoleSettings::default();
    }
    let provider = match &fields[F_PROVIDER] {
        SettingsField::Selector { options, index, .. } => options.get(*index).cloned().unwrap_or_default(),
        _ => String::new(),
    };
    let api_key = match &fields[F_API_KEY] {
        SettingsField::Secret { value, .. } => value.clone(),
        _ => String::new(),
    };
    let model = match &fields[F_MODEL] {
        SettingsField::Selector { options, index, .. } => options.get(*index).cloned().unwrap_or_default(),
        _ => String::new(),
    };
    let base_url = match &fields[F_BASE_URL] {
        SettingsField::Text { value, .. } => value.clone(),
        _ => String::new(),
    };

    let mut provider_params = HashMap::new();
    for (i, ps) in provider_settings.iter().enumerate() {
        let fi = NUM_BASE_FIELDS + i;
        if fi < fields.len() {
            let val = field_to_param_value(&fields[fi], ps);
            if !val.is_empty() {
                provider_params.insert(ps.key.clone(), val);
            }
        }
    }

    RoleSettings { provider, api_key, model, base_url, provider_params }
}

// ---------------------------------------------------------------------------
// Gateway queries (all go through GatewayConnection)
// ---------------------------------------------------------------------------

pub fn query_list_providers(gw: &mut crate::settings::GatewayConnection) -> std::io::Result<Vec<ProviderMeta>> {
    let resp = gw.request(&serde_json::json!({"type": "list-providers"}))?;
    if resp.status != 200 {
        let msg = resp.error.map(|e| e.message).unwrap_or_default();
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("gateway {}: {}", resp.status, msg)));
    }
    let body = resp.body.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no body"))?;
    let providers: Vec<ProviderMeta> = serde_json::from_value(
        body.get("providers").cloned().unwrap_or(serde_json::Value::Null)
    ).unwrap_or_default();
    Ok(providers)
}

pub fn query_models(gw: &mut crate::settings::GatewayConnection, provider: &str, api_key: &str, base_url: &str) -> std::io::Result<Vec<ModelEntry>> {
    let mut req = serde_json::json!({"type": "models", "provider": provider});
    let mut cfg = serde_json::json!({"id": provider});
    if !api_key.is_empty() {
        cfg["api_key"] = serde_json::json!(api_key);
    }
    if !base_url.is_empty() {
        cfg["base_url"] = serde_json::json!(base_url);
    }
    if cfg.as_object().map_or(false, |m| m.len() > 1) {
        req["config"] = cfg;
    }
    let resp = gw.request(&req)?;
    if resp.status != 200 {
        let msg = resp.error.map(|e| e.message).unwrap_or_default();
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("gateway {}: {}", resp.status, msg)));
    }
    let body = resp.body.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no body"))?;
    let models: Vec<ModelEntry> = serde_json::from_value(
        body.get("models").cloned().unwrap_or(serde_json::Value::Null)
    ).unwrap_or_default();
    Ok(models)
}

pub fn query_provider_info(gw: &mut crate::settings::GatewayConnection, provider: &str) -> std::io::Result<ProviderInfo> {
    let resp = gw.request(&serde_json::json!({"type": "provider-info", "provider": provider}))?;
    if resp.status != 200 {
        let msg = resp.error.map(|e| e.message).unwrap_or_default();
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("provider-info {}: {}", resp.status, msg)));
    }
    let body = resp.body.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no body"))?;
    serde_json::from_value(body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn validate_parameters(
    gw: &mut crate::settings::GatewayConnection,
    provider: &str,
    api_key: &str,
    model: &str,
    base_url: &str,
) -> std::io::Result<serde_json::Value> {
    let mut config = serde_json::json!({"id": provider, "model": model});
    if !api_key.is_empty() {
        config["api_key"] = serde_json::Value::String(api_key.to_string());
    }
    if !base_url.is_empty() {
        config["base_url"] = serde_json::Value::String(base_url.to_string());
    }
    let resp = gw.request(&serde_json::json!({
        "type": "validate-parameters",
        "provider": provider,
        "config": config,
    }))?;
    if resp.status != 200 {
        let msg = resp.error.map(|e| e.message).unwrap_or_default();
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("validate {}: {}", resp.status, msg)));
    }
    Ok(resp.body.unwrap_or(serde_json::Value::Null))
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn settings_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let dir = std::path::Path::new(&home).join(".di");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        crate::logging::log_event(&format!("failed to create ~/.di: {}", e));
    }
    dir.join("provider-settings.json")
}

pub fn load_all_settings() -> AllSettings {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            match serde_json::from_str(&s) {
                Ok(settings) => settings,
                Err(e) => {
                    crate::logging::log_event(&format!("corrupt settings file, using defaults: {}", e));
                    AllSettings::default()
                }
            }
        }
        Err(_) => AllSettings::default(),
    }
}

pub fn save_all_settings_to_disk(settings: &AllSettings) -> std::io::Result<()> {
    let path = settings_path();
    let json = serde_json::to_string_pretty(settings)?;
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Gateway push (reuses connection when available)
// ---------------------------------------------------------------------------

pub fn push_role_to_gateway(gw: &mut crate::settings::GatewayConnection, role: &str, rs: &RoleSettings) -> std::io::Result<()> {
    let provider_key = format!("{}:{}", rs.provider, role);

    let mut config = serde_json::json!({
        "id": rs.provider,
        "model": rs.model,
        "api_key": if rs.api_key.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(rs.api_key.clone()) },
        "base_url": if rs.base_url.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(rs.base_url.clone()) },
    });

    for (key, val) in &rs.provider_params {
        config[key] = string_to_json_value(val);
    }

    let msg = serde_json::json!({
        "type": "set-provider",
        "provider": provider_key,
        "config": config,
    });

    let _resp = gw.request(&msg)?;
    Ok(())
}

pub fn push_all_to_gateway() {
    let all = load_all_settings();
    let mut gw = crate::settings::GatewayConnection::new();
    if let Err(e) = gw.ensure_connected() {
        crate::logging::log_event(&format!("push_all_to_gateway: connect failed: {}", e));
        return;
    }
    for role in ROLES {
        if let Some(rs) = all.roles.get(*role) {
            if !rs.provider.is_empty() {
                if let Err(e) = push_role_to_gateway(&mut gw, role, rs) {
                    crate::logging::log_event(&format!("push_all_to_gateway: {} failed: {}", role, e));
                }
            }
        }
    }
}

/// Build SetProviderConfig messages from current settings for all configured roles.
pub fn build_provider_config_messages(all: &AllSettings) -> Vec<crate::message::FrontendMessage> {
    let mut messages = Vec::new();
    for role in ROLES {
        if let Some(rs) = all.roles.get(*role) {
            if !rs.provider.is_empty() && !rs.model.is_empty() {
                let params = rs.provider_params.iter().map(|(k, v)| {
                    (k.clone(), string_to_json_value(v))
                }).collect();
                messages.push(crate::message::FrontendMessage::SetProviderConfig {
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
    messages
}

/// Build a SetObserverConfig message from the observer behavior settings.
pub fn build_observer_config_message(all: &AllSettings) -> Option<crate::message::FrontendMessage> {
    let beh = all.behaviors.get("observer")?;
    let observer_role = all.roles.get("observer");
    Some(crate::message::FrontendMessage::SetObserverConfig {
        enabled: beh.enabled.unwrap_or(false),
        use_llm_observations: beh.enabled.unwrap_or(false),
        watcher_frequency: beh.observer_turns.unwrap_or(2) as usize,
        critic_frequency: beh.observer_critic_frequency.unwrap_or(6) as usize,
        verbose: beh.observer_verbose.unwrap_or(false),
        token_threshold: beh.observer_token_threshold.unwrap_or(15000) as usize,
        buffer_activation: beh.observer_buffer_activation.map(|v| v.round() as usize).unwrap_or(3),
        block_after: beh.observer_block_after.unwrap_or(0.7) as f32,
        reflection_enabled: beh.observer_reflection_enabled.unwrap_or(true),
        reflection_token_threshold: beh.observer_reflection_token_threshold.unwrap_or(10000) as usize,
        procedural_monotonicity_enabled: true,
        ast_guided_memory_enabled: true,
        adaptive_cooldown_enabled: true,
        latency_budget_ms: 0,
        permissive_buffer_size: 3,
        observer_provider: observer_role.as_ref().map(|r| r.provider.clone()).filter(|p| !p.is_empty()),
        observer_model_id: observer_role.as_ref().map(|r| r.model.clone()).filter(|m| !m.is_empty()),
    })
}

// ---------------------------------------------------------------------------
// High-level field builders (need GatewayConnection for model queries)
// ---------------------------------------------------------------------------

pub fn build_role_fields(
    rs: &RoleSettings,
    providers: &[ProviderMeta],
    gw: &mut crate::settings::GatewayConnection,
    gateway_ok: bool,
) -> (Vec<SettingsField>, Vec<ModelEntry>, Option<ProviderInfo>, Option<String>) {
    let (provider_index, provider_id, provider_options, provider_labels) = if rs.provider.is_empty() {
        let mut options = vec![String::new()];
        let mut labels = vec!["(none)".to_string()];
        options.extend(providers.iter().map(|p| p.id.clone()));
        labels.extend(providers.iter().map(|p| format!("{} ({})", p.label, p.id)));
        (0, String::new(), options, labels)
    } else {
        let idx = providers.iter().position(|p| p.id == rs.provider).unwrap_or(0);
        let id = providers.get(idx).map(|p| p.id.clone()).unwrap_or_else(|| rs.provider.clone());
        let labels: Vec<String> = providers.iter().map(|p| format!("{} ({})", p.label, p.id)).collect();
        let options: Vec<String> = providers.iter().map(|p| p.id.clone()).collect();
        (idx, id, options, labels)
    };

    let (models, model_index) = if gateway_ok && !provider_id.is_empty() {
        match query_models(gw, &provider_id, &rs.api_key, &rs.base_url) {
            Ok(m) => {
                let idx = m.iter().position(|m| m.id == rs.model).unwrap_or(0);
                (m, idx)
            }
            Err(e) => {
                return (
                    build_minimal_fields(rs, providers),
                    Vec::new(),
                    None,
                    Some(format!("Failed to load models: {}", e)),
                );
            }
        }
    } else if rs.model.is_empty() {
        (Vec::new(), 0)
    } else {
        let saved = rs.model.clone();
        (vec![ModelEntry { id: saved.clone(), name: Some(saved), context_window: None, max_tokens: None, supports_thinking: None }], 0)
    };

    let model_options: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    let model_labels: Vec<String> = models.iter()
        .map(|m| m.name.as_deref().unwrap_or(&m.id).to_string())
        .collect();

    let mut fields = vec![
        SettingsField::Selector {
            label: "Provider".to_string(),
            options: provider_options,
            labels: provider_labels,
            index: provider_index,
        },
        SettingsField::Secret {
            label: "API Key".to_string(),
            value: rs.api_key.clone(),
        },
        SettingsField::Selector {
            label: "Model".to_string(),
            options: model_options,
            labels: model_labels,
            index: model_index,
        },
        SettingsField::Text {
            label: "Base URL".to_string(),
            value: rs.base_url.clone(),
        },
    ];

    let provider_info = if gateway_ok && !provider_id.is_empty() {
        match query_provider_info(gw, &provider_id) {
            Ok(info) => Some(info),
            Err(e) => {
                return (fields, models, None, Some(format!("Provider params unavailable: {}", e)));
            }
        }
    } else {
        None
    };

    if let Some(ref info) = provider_info {
        for ps in &info.settings {
            let field = provider_setting_to_field(ps, rs.provider_params.get(&ps.key));
            fields.push(field);
        }
    }

    (fields, models, provider_info, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_settings_default() {
        let settings = AllSettings::default();
        assert!(settings.roles.is_empty());
        assert_eq!(settings.theme, "copper-cobalt-dimmed");
    }

    #[test]
    fn test_role_settings_default() {
        let rs = RoleSettings::default();
        assert!(rs.provider.is_empty());
        assert!(rs.model.is_empty());
        assert!(rs.api_key.is_empty());
    }

    #[test]
    fn test_load_save_roundtrip() {
        let mut settings = AllSettings::default();
        let mut rs = RoleSettings::default();
        rs.provider = "test-provider".to_string();
        rs.model = "test-model".to_string();
        rs.api_key = "test-key".to_string();
        settings.roles.insert("observer".to_string(), rs.clone());

        // Serialize
        let json = serde_json::to_string_pretty(&settings).unwrap();
        let parsed: AllSettings = serde_json::from_str(&json).unwrap();

        let loaded_rs = parsed.roles.get("observer").unwrap();
        assert_eq!(loaded_rs.provider, "test-provider");
        assert_eq!(loaded_rs.model, "test-model");
        assert_eq!(loaded_rs.api_key, "test-key");
    }

    #[test]
    fn test_load_corrupt_json_returns_default() {
        let result: AllSettings = serde_json::from_str("{invalid json").unwrap_or_default();
        assert!(result.roles.is_empty());
    }

    #[test]
    fn test_build_provider_config_messages() {
        let mut settings = AllSettings::default();
        let mut rs = RoleSettings::default();
        rs.provider = "anthropic".to_string();
        rs.model = "claude-3".to_string();
        settings.roles.insert("observer".to_string(), rs);

        let msgs = build_provider_config_messages(&settings);
        assert_eq!(msgs.len(), 1);
    }
}
