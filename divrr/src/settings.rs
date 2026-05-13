use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;

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

/// Convert a string value to a properly typed JSON value (bool, number, or string).
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
    status: i64,
    body: Option<serde_json::Value>,
    error: Option<GatewayError>,
}

#[derive(Deserialize)]
struct GatewayError {
    message: String,
}

// ---------------------------------------------------------------------------
// Persistent gateway connection — one connection for the whole settings dialog
// ---------------------------------------------------------------------------

pub struct GatewayConnection {
    socket_path: String,
    reader: Option<std::io::BufReader<UnixStream>>,
}

impl GatewayConnection {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let socket_path = std::env::var("DIRAC_API_GATEWAY_SOCKET")
            .unwrap_or_else(|_| format!("{}/.dirac/api-gateway.sock", home));
        Self { socket_path, reader: None }
    }

    pub fn ensure_connected(&mut self) -> std::io::Result<()> {
        if self.reader.is_some() {
            return Ok(());
        }
        let stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(std::time::Duration::from_secs(3)))?;
        self.reader = Some(std::io::BufReader::new(stream));
        Ok(())
    }

    pub fn request(&mut self, req: &serde_json::Value) -> std::io::Result<GatewayResponse> {
        self.ensure_connected()?;
        let reader = self.reader.as_mut().unwrap();
        let stream = reader.get_mut();

        let json = serde_json::to_string(req)?;
        stream.write_all(json.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()?;

        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // Connection closed by gateway — reconnect and retry once
                    self.reader = None;
                    self.ensure_connected()?;
                    let reader = self.reader.as_mut().unwrap();
                    let stream = reader.get_mut();
                    stream.write_all(json.as_bytes())?;
                    stream.write_all(b"\n")?;
                    stream.flush()?;
                    line.clear();
                    reader.read_line(&mut line)?;
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "empty response after reconnect"));
                    }
                    return serde_json::from_str(trimmed)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e));
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }
                    match serde_json::from_str::<GatewayResponse>(trimmed) {
                        Ok(resp) => return Ok(resp),
                        Err(_) => continue, // skip non-JSON lines
                    }
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
                        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "gateway read timed out"));
                    }
                    self.reader = None;
                    return Err(e);
                }
            }
        }
    }

}

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
                if value.is_empty() {
                    String::new()
                } else {
                    value.clone()
                }
            }
            SettingsField::Secret { value, .. } => {
                if value.is_empty() {
                    String::new()
                } else {
                    "*".repeat(value.len().min(30))
                }
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
pub struct AllSettings {
    #[serde(flatten)]
    pub roles: HashMap<String, RoleSettings>,
}

// ---------------------------------------------------------------------------
// Settings state
// ---------------------------------------------------------------------------

pub struct SettingsState {
    // Role selector
    pub role_options: Vec<String>,
    pub role_index: usize,

    // Per-role fields
    pub fields: Vec<SettingsField>,

    // Cursor: 0 = role tabs row, 1..N = fields
    pub cursor: usize,

    // Selection modal state
    pub selector_open: bool,
    pub selector_cursor: usize,
    pub selector_filter: String,
    pub selector_filtered_indices: Vec<usize>,

    // UI state
    pub saved: bool,
    pub error: Option<String>,
    pub gateway_available: bool,

    // Secret edit modal (Tab on API key)
    pub secret_edit_open: bool,
    pub secret_edit_buffer: String,
    pub secret_edit_cursor: usize,

    // Cached gateway data
    pub provider_metas: Vec<ProviderMeta>,
    pub model_entries: Vec<ModelEntry>,
    pub provider_info: Option<ProviderInfo>,

    // All saved role settings
    pub all_settings: AllSettings,

    // Loading state
    pub loading: bool,
    /// If set, the app should spawn the described async operation after this key handler returns.
    pub pending_async: Option<PendingAsyncOp>,
    /// Monotonic counter for async operations — used to discard stale results.
    pub async_op_seq: u64,
}

/// Describes an async gateway operation the app should spawn.
#[derive(Debug)]
pub enum PendingAsyncOp {
    /// Provider changed: query models + provider_info + rebuild fields
    ProviderChanged { seq: u64, rs: RoleSettings, providers: Vec<ProviderMeta>, gateway_available: bool },
    /// Role switched: build_role_fields for the new role
    RoleSwitched { seq: u64, rs: RoleSettings, providers: Vec<ProviderMeta>, gateway_available: bool },
    /// Save: push all role configs to gateway + validate
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
    },
    ProviderChanged {
        seq: u64,
        fields: Vec<SettingsField>,
        model_entries: Vec<ModelEntry>,
        provider_info: Option<ProviderInfo>,
    },
    RoleSwitched {
        seq: u64,
        fields: Vec<SettingsField>,
        model_entries: Vec<ModelEntry>,
        provider_info: Option<ProviderInfo>,
    },
    Saved {
        error: Option<String>,
    },
}

// Field offsets within cursor (cursor 0 = role tabs, 1+ = fields)
const F_PROVIDER: usize = 0;
const F_API_KEY: usize = 1;
const F_MODEL: usize = 2;
const F_BASE_URL: usize = 3;
const NUM_BASE_FIELDS: usize = 4;

impl SettingsState {
    /// Create settings state from file config only — no gateway calls.
    /// Gateway data is loaded asynchronously and applied via apply_initial_load().
    pub fn new_empty() -> Self {
        let mut all_settings = load_all_settings();
        for role in ROLES {
            all_settings.roles.entry(role.to_string()).or_default();
        }

        let role_options: Vec<String> = ROLES.iter().map(|r| r.to_string()).collect();
        let rs = all_settings.roles.get(ROLES[0]).cloned().unwrap_or_default();

        // Build minimal fields without gateway data
        let fields = vec![
            SettingsField::Selector {
                label: "Provider".to_string(),
                options: Vec::new(),
                labels: Vec::new(),
                index: 0,
            },
            SettingsField::Secret {
                label: "API Key".to_string(),
                value: rs.api_key.clone(),
            },
            SettingsField::Selector {
                label: "Model".to_string(),
                options: Vec::new(),
                labels: Vec::new(),
                index: 0,
            },
            SettingsField::Text {
                label: "Base URL".to_string(),
                value: rs.base_url.clone(),
            },
        ];

        Self {
            role_options,
            role_index: 0,
            fields,
            cursor: 0,
            selector_open: false,
            selector_cursor: 0,
            selector_filter: String::new(),
            selector_filtered_indices: Vec::new(),
            saved: false,
            error: None,
            gateway_available: false,
            secret_edit_open: false,
            secret_edit_buffer: String::new(),
            secret_edit_cursor: 0,
            provider_metas: Vec::new(),
            model_entries: Vec::new(),
            provider_info: None,
            all_settings,
            loading: true,
            pending_async: None,
            async_op_seq: 0,
        }
    }

    /// Apply the initial gateway load result (providers, models, fields).
    pub fn apply_initial_load(&mut self, result: SettingsLoadResult) {
        if let SettingsLoadResult::Initial { providers, fields, model_entries, provider_info, gateway_available } = result {
            self.provider_metas = providers;
            self.fields = fields;
            self.model_entries = model_entries;
            self.provider_info = provider_info;
            self.gateway_available = gateway_available;
            if !gateway_available {
                self.error = Some("API gateway not available".into());
            }
        }
        self.loading = false;
    }

    /// Apply a provider change result.
    pub fn apply_provider_changed(&mut self, result: SettingsLoadResult) {
        if let SettingsLoadResult::ProviderChanged { seq, fields, model_entries, provider_info } = result {
            if seq == self.async_op_seq {
                self.fields = fields;
                self.model_entries = model_entries;
                self.provider_info = provider_info;
                self.flush_fields_to_settings();
                self.loading = false;
            }
        }
    }

    /// Apply a role switch result.
    pub fn apply_role_switched(&mut self, result: SettingsLoadResult) {
        if let SettingsLoadResult::RoleSwitched { seq, fields, model_entries, provider_info } = result {
            if seq == self.async_op_seq {
                self.fields = fields;
                self.model_entries = model_entries;
                self.provider_info = provider_info;
                self.flush_fields_to_settings();
                self.loading = false;
            }
        }
    }

    /// Apply a save result. Messages are sent synchronously by save(); this only applies the error/status.
    pub fn apply_saved(&mut self, result: SettingsLoadResult) {
        if let SettingsLoadResult::Saved { error } = result {
            let ok = error.is_none();
            self.error = error;
            if ok {
                self.saved = true;
            }
        }
    }

    fn current_role(&self) -> &str {
        self.role_options.get(self.role_index).map(|s| s.as_str()).unwrap_or(ROLES[0])
    }

    pub fn field_offset(&self) -> usize {
        // cursor 0 = role tabs, 1.. = field index 0..
        if self.cursor == 0 { 0 } else { self.cursor - 1 }
    }

    // -- Navigation --

    pub fn move_up(&mut self) {
        if self.selector_open {
            if self.selector_cursor > 0 {
                self.selector_cursor -= 1;
            }
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selector_open {
            let count = self.selector_option_count();
            if self.selector_cursor < count.saturating_sub(1) {
                self.selector_cursor += 1;
            }
            return;
        }
        let max = self.fields.len(); // cursor 0..fields.len()
        if self.cursor < max {
            self.cursor += 1;
        }
    }

    pub fn select_left(&mut self) {
        if self.selector_open { return; }
        if self.cursor == 0 {
            if self.role_index > 0 {
                self.flush_fields_to_settings();
                self.role_index -= 1;
                self.load_role_fields();
            }
            return;
        }
        let fo = self.field_offset();
        let is_provider = fo == F_PROVIDER;
        if let SettingsField::Selector { index, .. } = &mut self.fields[fo] {
            if *index > 0 {
                *index -= 1;
                self.saved = false;
                if is_provider {
                    self.on_provider_changed();
                }
            }
        }
    }

    pub fn select_right(&mut self) {
        if self.selector_open { return; }
        if self.cursor == 0 {
            if self.role_index < self.role_options.len() - 1 {
                self.flush_fields_to_settings();
                self.role_index += 1;
                self.load_role_fields();
            }
            return;
        }
        let fo = self.field_offset();
        let is_provider = fo == F_PROVIDER;
        if let SettingsField::Selector { index, options, .. } = &mut self.fields[fo] {
            if !options.is_empty() && *index < options.len() - 1 {
                *index += 1;
                self.saved = false;
                if is_provider {
                    self.on_provider_changed();
                }
            }
        }
    }

    pub fn open_selector(&mut self) {
        if self.cursor == 0 { return; }
        let fo = self.field_offset();
        if !matches!(self.fields[fo].kind(), FieldKind::Selector) { return; }

        let current_index = match &self.fields[fo] {
            SettingsField::Selector { index, .. } => *index,
            _ => 0,
        };
        self.selector_open = true;
        self.selector_filter.clear();
        self.rebuild_filtered_indices();
        // Position cursor at current selection in the filtered list
        self.selector_cursor = self.selector_filtered_indices.iter()
            .position(|&i| i == current_index)
            .unwrap_or(0);
    }

    // Secret edit modal methods

    pub fn open_secret_edit(&mut self) {
        if self.cursor == 0 { return; }
        let fo = self.field_offset();
        if !matches!(self.fields[fo].kind(), FieldKind::Secret) { return; }
        self.secret_edit_buffer = match &self.fields[fo] {
            SettingsField::Secret { value, .. } => value.clone(),
            _ => String::new(),
        };
        self.secret_edit_cursor = self.secret_edit_buffer.len();
        self.secret_edit_open = true;
    }

    pub fn confirm_secret_edit(&mut self) {
        if !self.secret_edit_open { return; }
        let fo = self.field_offset();
        if let SettingsField::Secret { value, .. } = &mut self.fields[fo] {
            *value = self.secret_edit_buffer.clone();
            self.saved = false;
        }
        self.secret_edit_open = false;
    }

    pub fn cancel_secret_edit(&mut self) {
        self.secret_edit_open = false;
    }

    pub fn secret_edit_type_char(&mut self, c: char) {
        self.secret_edit_buffer.insert(self.secret_edit_cursor, c);
        self.secret_edit_cursor += c.len_utf8();
    }

    pub fn secret_edit_backspace(&mut self) {
        if self.secret_edit_cursor > 0 {
            // Find the previous char boundary
            let prev = self.secret_edit_buffer[..self.secret_edit_cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.secret_edit_buffer.drain(prev..self.secret_edit_cursor);
            self.secret_edit_cursor = prev;
        }
    }

    pub fn secret_edit_delete(&mut self) {
        if self.secret_edit_cursor < self.secret_edit_buffer.len() {
            let next = self.secret_edit_buffer[self.secret_edit_cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.secret_edit_cursor + i)
                .unwrap_or(self.secret_edit_buffer.len());
            self.secret_edit_buffer.drain(self.secret_edit_cursor..next);
        }
    }

    pub fn secret_edit_left(&mut self) {
        if self.secret_edit_cursor > 0 {
            let prev = self.secret_edit_buffer[..self.secret_edit_cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.secret_edit_cursor = prev;
        }
    }

    pub fn secret_edit_right(&mut self) {
        if self.secret_edit_cursor < self.secret_edit_buffer.len() {
            let next = self.secret_edit_buffer[self.secret_edit_cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.secret_edit_cursor + i)
                .unwrap_or(self.secret_edit_buffer.len());
            self.secret_edit_cursor = next;
        }
    }

    pub fn secret_edit_home(&mut self) {
        self.secret_edit_cursor = 0;
    }

    pub fn secret_edit_end(&mut self) {
        self.secret_edit_cursor = self.secret_edit_buffer.len();
    }

    pub fn confirm_selector(&mut self) {
        if !self.selector_open { return; }
        let fo = self.field_offset();
        let is_provider = fo == F_PROVIDER;
        // Map filtered cursor back to real index
        let real_index = self.selector_filtered_indices
            .get(self.selector_cursor)
            .copied()
            .unwrap_or(0);
        if let SettingsField::Selector { index, .. } = &mut self.fields[fo] {
            *index = real_index;
            self.saved = false;
            if is_provider {
                self.on_provider_changed();
            }
        }
        self.selector_open = false;
    }

    pub fn cancel_selector(&mut self) {
        self.selector_open = false;
    }

    fn rebuild_filtered_indices(&mut self) {
        let fo = self.field_offset();
        let filter_lower = self.selector_filter.to_lowercase();
        self.selector_filtered_indices = match &self.fields.get(fo) {
            Some(SettingsField::Selector { options, labels, .. }) => {
                options.iter().enumerate()
                    .filter(|(i, opt)| {
                        if filter_lower.is_empty() { return true; }
                        let label = labels.get(*i).map(|s| s.as_str()).unwrap_or(opt.as_str());
                        opt.to_lowercase().contains(&filter_lower)
                            || label.to_lowercase().contains(&filter_lower)
                    })
                    .map(|(i, _)| i)
                    .collect()
            }
            _ => Vec::new(),
        };
        if self.selector_cursor >= self.selector_filtered_indices.len() {
            self.selector_cursor = 0;
        }
    }

    pub fn selector_option_count(&self) -> usize {
        self.selector_filtered_indices.len()
    }

    pub fn selector_label_at(&self, filtered_index: usize) -> String {
        let fo = self.field_offset();
        let real_index = self.selector_filtered_indices.get(filtered_index).copied().unwrap_or(0);
        match &self.fields.get(fo) {
            Some(SettingsField::Selector { labels, options, .. }) => {
                if real_index < labels.len() {
                    labels[real_index].clone()
                } else if real_index < options.len() {
                    options[real_index].clone()
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    }

    pub fn type_char(&mut self, c: char) {
        if self.selector_open {
            self.selector_filter.push(c);
            self.rebuild_filtered_indices();
            return;
        }
        if self.cursor == 0 { return; }
        let fo = self.field_offset();
        // Don't allow inline typing on Secret fields — must use Tab modal
        match &mut self.fields[fo] {
            SettingsField::Text { value, .. } => {
                value.push(c);
                self.saved = false;
            }
            _ => {}
        }
    }

    pub fn backspace(&mut self) {
        if self.selector_open {
            self.selector_filter.pop();
            self.rebuild_filtered_indices();
            return;
        }
        if self.cursor == 0 { return; }
        let fo = self.field_offset();
        match &mut self.fields[fo] {
            SettingsField::Text { value, .. } => {
                value.pop();
                self.saved = false;
            }
            _ => {}
        }
    }

    fn on_provider_changed(&mut self) {
        self.flush_fields_to_settings();
        self.error = None;
        self.loading = true;
        let provider_id = match &self.fields[F_PROVIDER] {
            SettingsField::Selector { options, index, .. } => {
                options.get(*index).cloned().unwrap_or_default()
            }
            _ => return,
        };

        let api_key = match &self.fields[F_API_KEY] {
            SettingsField::Secret { value, .. } => value.clone(),
            _ => String::new(),
        };
        let base_url = match self.fields.get(F_BASE_URL) {
            Some(SettingsField::Text { value, .. }) => value.clone(),
            _ => String::new(),
        };

        // Reset model and dynamic fields while loading
        self.fields.truncate(NUM_BASE_FIELDS);
        self.fields[F_MODEL] = SettingsField::Selector {
            label: "Model".to_string(),
            options: Vec::new(),
            labels: Vec::new(),
            index: 0,
        };
        self.provider_info = None;

        self.async_op_seq += 1;
        self.pending_async = Some(PendingAsyncOp::ProviderChanged {
            seq: self.async_op_seq,
            rs: RoleSettings {
                provider: provider_id,
                api_key,
                model: String::new(),
                base_url,
                provider_params: HashMap::new(),
            },
            providers: self.provider_metas.clone(),
            gateway_available: self.gateway_available,
        });
    }

    fn flush_fields_to_settings(&mut self) {
        let role = self.current_role().to_string();
        let provider_settings: Vec<ProviderSetting> = self.provider_info
            .as_ref()
            .map(|info| info.settings.clone())
            .unwrap_or_default();
        let rs = gather_role_settings(&self.fields, &provider_settings);
        self.all_settings.roles.insert(role, rs);
    }

    fn load_role_fields(&mut self) {
        self.loading = true;
        self.async_op_seq += 1;
        let role = self.current_role();
        let rs = self.all_settings.roles.get(role).cloned().unwrap_or_default();
        self.pending_async = Some(PendingAsyncOp::RoleSwitched {
            seq: self.async_op_seq,
            rs,
            providers: self.provider_metas.clone(),
            gateway_available: self.gateway_available,
        });
    }

    // -- Save --

    /// Save settings and return FrontendMessages to send to di-core.
    /// Gateway push and validation happen asynchronously via pending_async.
    pub fn save(&mut self) -> Vec<crate::message::FrontendMessage> {
        if self.selector_open {
            self.confirm_selector();
            return Vec::new();
        }
        if self.secret_edit_open {
            self.confirm_secret_edit();
            return Vec::new();
        }

        self.flush_fields_to_settings();

        let mut error_msgs = Vec::new();
        for role in ROLES {
            if let Some(rs) = self.all_settings.roles.get(*role) {
                if rs.provider.is_empty() { continue; }
                if rs.model.is_empty() && self.gateway_available {
                    error_msgs.push(format!("{}: model required", role_label(role)));
                }
            }
        }

        if !error_msgs.is_empty() {
            self.error = Some(error_msgs.join(", "));
            return Vec::new();
        }

        // Build SetProviderConfig messages for all configured roles
        let mut messages = Vec::new();
        for role in ROLES {
            if let Some(rs) = self.all_settings.roles.get(*role) {
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

        // Defer gateway push + validation to background
        self.pending_async = Some(PendingAsyncOp::Save { all_settings: self.all_settings.clone() });
        self.saved = true;

        messages
    }
}

// ---------------------------------------------------------------------------
// Field construction helpers
// ---------------------------------------------------------------------------

pub fn build_role_fields(
    rs: &RoleSettings,
    providers: &[ProviderMeta],
    gw: &mut GatewayConnection,
    gateway_ok: bool,
) -> (Vec<SettingsField>, Vec<ModelEntry>, Option<ProviderInfo>) {
    let provider_index = providers.iter().position(|p| p.id == rs.provider).unwrap_or(0);
    let provider_id = providers.get(provider_index)
        .map(|p| p.id.clone())
        .unwrap_or_else(|| rs.provider.clone());

    let provider_labels: Vec<String> = providers.iter()
        .map(|p| format!("{} ({})", p.label, p.id))
        .collect();
    let provider_options: Vec<String> = providers.iter().map(|p| p.id.clone()).collect();

    let (models, model_index) = if gateway_ok && !provider_id.is_empty() {
        let models = match query_models(gw, &provider_id, &rs.api_key) {
            Ok(m) => m,
            Err(_) => Vec::new(),
        };
        let idx = models.iter().position(|m| m.id == rs.model).unwrap_or(0);
        (models, idx)
    } else {
        (Vec::new(), 0)
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

    // Fetch provider-info and append dynamic parameter fields
    let provider_info = if gateway_ok && !provider_id.is_empty() {
        query_provider_info(gw, &provider_id).ok()
    } else {
        None
    };

    if let Some(ref info) = provider_info {
        for ps in &info.settings {
            let field = provider_setting_to_field(ps, rs.provider_params.get(&ps.key));
            fields.push(field);
        }
    }

    (fields, models, provider_info)
}

fn gather_role_settings(fields: &[SettingsField], provider_settings: &[ProviderSetting]) -> RoleSettings {
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

    // Gather dynamic provider params (fields after F_BASE_URL)
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

fn provider_setting_to_field(ps: &ProviderSetting, current_value: Option<&String>) -> SettingsField {
    let val = current_value.cloned().unwrap_or_else(|| {
        ps.default.as_ref().map(|v| value_to_string(v)).unwrap_or_default()
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
            let options: Vec<String> = ps.options.iter().map(|o| o.value.clone()).collect();
            let labels: Vec<String> = ps.options.iter()
                .map(|o| o.label.clone().unwrap_or_else(|| o.value.clone()))
                .collect();
            let index = options.iter().position(|o| o == &val).unwrap_or(0);
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

fn value_to_string(v: &serde_json::Value) -> String {
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
            // For sliders/numbers, validate the value
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

// ---------------------------------------------------------------------------
// Gateway queries (all go through GatewayConnection)
// ---------------------------------------------------------------------------

pub fn query_list_providers(gw: &mut GatewayConnection) -> std::io::Result<Vec<ProviderMeta>> {
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

pub fn query_models(gw: &mut GatewayConnection, provider: &str, api_key: &str) -> std::io::Result<Vec<ModelEntry>> {
    let mut req = serde_json::json!({"type": "models", "provider": provider});
    if !api_key.is_empty() {
        req["config"] = serde_json::json!({"id": provider, "api_key": api_key});
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

pub fn query_provider_info(gw: &mut GatewayConnection, provider: &str) -> std::io::Result<ProviderInfo> {
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
    gw: &mut GatewayConnection,
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
    let dir = std::path::Path::new(&home).join(".dirac");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("provider-settings.json")
}

pub fn load_all_settings() -> AllSettings {
    let path = settings_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_all_settings_to_disk(settings: &AllSettings) -> std::io::Result<()> {
    let path = settings_path();
    let json = serde_json::to_string_pretty(settings)?;
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Gateway push (reuses connection when available)
// ---------------------------------------------------------------------------

pub fn push_role_to_gateway(gw: &mut GatewayConnection, role: &str, rs: &RoleSettings) -> std::io::Result<()> {
    let provider_key = format!("{}:{}", rs.provider, role);

    let mut config = serde_json::json!({
        "id": rs.provider,
        "model": rs.model,
        "api_key": if rs.api_key.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(rs.api_key.clone()) },
        "base_url": if rs.base_url.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(rs.base_url.clone()) },
    });

    // Include provider-specific params
    for (key, val) in &rs.provider_params {
        // Try to parse numbers/bools for proper typing
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
    let mut gw = GatewayConnection::new();
    if gw.ensure_connected().is_err() { return; }
    for role in ROLES {
        if let Some(rs) = all.roles.get(*role) {
            if !rs.provider.is_empty() {
                let _ = push_role_to_gateway(&mut gw, role, rs);
            }
        }
    }
}
