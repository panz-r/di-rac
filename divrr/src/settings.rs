use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;

// Re-export all types and free functions from settings_model
pub use crate::settings_model::{
    ROLES, role_label, string_to_json_value,
    ProviderMeta, ModelEntry, ProviderInfo,
    FieldKind, SettingsField,
    RoleSettings, RoleBehaviorSettings, AllSettings, SettingsPanel,
    PendingAsyncOp, SettingsLoadResult,
    F_PROVIDER,
    build_provider_config_messages, build_role_fields, build_role_behavior_fields,
    build_theme_fields, load_all_settings, save_all_settings_to_disk,
    push_role_to_gateway, push_all_to_gateway, validate_parameters,
    query_list_providers,
    build_minimal_base_fields, gather_role_settings,
};

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

    pub fn request(&mut self, req: &serde_json::Value) -> std::io::Result<crate::settings_model::GatewayResponse> {
        self.ensure_connected()?;
        let reader = self.reader.as_mut().unwrap();
        let stream = reader.get_mut();

        let json = serde_json::to_string(req)?;
        if let Err(e) = stream.write_all(json.as_bytes()).and_then(|_| stream.write_all(b"\n")).and_then(|_| stream.flush()) {
            self.reader = None;
            return Err(e);
        }

        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    self.reader = None;
                    self.ensure_connected()?;
                    let reader = self.reader.as_mut().unwrap();
                    let stream = reader.get_mut();
                    if let Err(e) = stream.write_all(json.as_bytes()).and_then(|_| stream.write_all(b"\n")).and_then(|_| stream.flush()) {
                        self.reader = None;
                        return Err(e);
                    }
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            self.reader = None;
                            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "gateway closed again"));
                        }
                        Err(e) => {
                            self.reader = None;
                            return Err(e);
                        }
                        Ok(_) => {}
                    }
                    if line.len() > 10 * 1024 * 1024 {
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "gateway response exceeds 10 MB"));
                    }
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
                    match serde_json::from_str::<crate::settings_model::GatewayResponse>(trimmed) {
                        Ok(resp) => return Ok(resp),
                        Err(_) => continue,
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

// ---------------------------------------------------------------------------
// Settings state
// ---------------------------------------------------------------------------

pub struct SettingsState {
    pub active_panel: SettingsPanel,
    pub role_options: Vec<String>,
    pub role_index: usize,
    pub fields: Vec<SettingsField>,
    pub cursor: usize,
    pub selector_open: bool,
    pub selector_cursor: usize,
    pub selector_filter: String,
    pub selector_filtered_indices: Vec<usize>,
    pub saved: bool,
    pub saving: bool,
    pub error: Option<String>,
    pub gateway_available: bool,
    pub secret_edit_open: bool,
    pub secret_edit_buffer: String,
    pub secret_edit_cursor: usize,
    pub provider_metas: Vec<ProviderMeta>,
    pub model_entries: Vec<ModelEntry>,
    pub provider_info: Option<ProviderInfo>,
    pub all_settings: AllSettings,
    pub loading: bool,
    pub pending_async: Option<PendingAsyncOp>,
    pub async_op_seq: u64,
}

impl SettingsState {
    pub fn new_empty() -> Self {
        let mut all_settings = load_all_settings();
        for role in ROLES {
            all_settings.roles.entry(role.to_string()).or_default();
        }

        let role_options: Vec<String> = ROLES.iter().map(|r| r.to_string()).collect();
        let rs = all_settings.roles.get(ROLES[0]).cloned().unwrap_or_default();
        let fields = build_minimal_base_fields(&rs);

        Self {
            active_panel: SettingsPanel::Provider,
            role_options,
            role_index: 0,
            fields,
            cursor: 0,
            selector_open: false,
            selector_cursor: 0,
            selector_filter: String::new(),
            selector_filtered_indices: Vec::new(),
            saved: false,
            saving: false,
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

    pub fn apply_initial_load(&mut self, result: SettingsLoadResult) {
        if let SettingsLoadResult::Initial { providers, fields, model_entries, provider_info, gateway_available, gateway_error } = result {
            self.provider_metas = providers;
            self.fields = fields;
            self.model_entries = model_entries;
            self.provider_info = provider_info;
            self.gateway_available = gateway_available;
            self.cursor = self.cursor.min(self.fields.len());
            if let Some(err) = gateway_error {
                self.error = Some(err);
            } else if !gateway_available {
                self.error = Some("API gateway not available".into());
            }
        }
        self.loading = false;
    }

    pub fn apply_provider_changed(&mut self, result: SettingsLoadResult) {
        if let SettingsLoadResult::ProviderChanged { seq, fields, model_entries, provider_info, gateway_error } = result {
            if seq == self.async_op_seq {
                self.fields = fields;
                self.model_entries = model_entries;
                self.provider_info = provider_info;
                self.error = gateway_error;
                self.cursor = self.cursor.min(self.fields.len());
                self.flush_fields_to_settings();
            }
            self.loading = false;
        }
    }

    pub fn apply_role_switched(&mut self, result: SettingsLoadResult) {
        if let SettingsLoadResult::RoleSwitched { seq, fields, model_entries, provider_info, gateway_error } = result {
            if seq == self.async_op_seq {
                self.fields = fields;
                self.model_entries = model_entries;
                self.provider_info = provider_info;
                self.error = gateway_error;
                self.cursor = self.cursor.min(self.fields.len());
                self.flush_fields_to_settings();
            }
            self.loading = false;
        }
    }

    pub fn apply_saved(&mut self, result: SettingsLoadResult) -> Vec<crate::message::FrontendMessage> {
        self.saving = false;
        if let SettingsLoadResult::Saved { error, messages } = result {
            let ok = error.is_none();
            self.error = error;
            if ok {
                self.saved = true;
                return messages;
            }
        }
        Vec::new()
    }

    pub fn switch_panel(&mut self) {
        self.selector_open = false;
        self.secret_edit_open = false;
        self.active_panel = match self.active_panel {
            SettingsPanel::Provider => SettingsPanel::Role,
            SettingsPanel::Role => SettingsPanel::Theme,
            SettingsPanel::Theme => SettingsPanel::Provider,
        };
        self.cursor = 0;
        self.rebuild_fields_for_panel();
    }

    fn rebuild_fields_for_panel(&mut self) {
        match self.active_panel {
            SettingsPanel::Provider => self.load_role_fields(),
            SettingsPanel::Role => {
                let rs = self.all_settings.behaviors
                    .get(&self.role_options[self.role_index])
                    .cloned()
                    .unwrap_or_default();
                let role = &self.role_options[self.role_index];
                self.fields = build_role_behavior_fields(role, &rs);
            }
            SettingsPanel::Theme => {
                self.fields = build_theme_fields(&self.all_settings.theme);
            }
        }
        if self.cursor > self.fields.len() {
            self.cursor = self.fields.len();
        }
    }

    fn load_role_fields(&mut self) {
        let role = &self.role_options[self.role_index];
        let rs = self.all_settings.roles.get(role).cloned().unwrap_or_default();
        self.fields = build_minimal_base_fields(&rs);

        if self.gateway_available {
            let seq = self.async_op_seq.wrapping_add(1);
            self.async_op_seq = seq;
            self.loading = true;
            self.pending_async = Some(PendingAsyncOp::ProviderChanged {
                seq,
                rs,
                providers: self.provider_metas.clone(),
                gateway_available: self.gateway_available,
            });
        }
    }

    pub fn field_offset(&self) -> usize {
        if self.cursor == 0 { 0 } else { self.cursor - 1 }
    }

    pub fn move_up(&mut self) {
        if self.selector_open {
            if self.selector_cursor > 0 {
                self.selector_cursor -= 1;
            }
            return;
        }
        if self.cursor > 1 && self.fields.is_empty() {
            self.cursor -= 1;
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
        let max = self.fields.len();
        if self.cursor < max {
            self.cursor += 1;
        }
    }

    pub fn select_left(&mut self) {
        self.error = None;
        if self.selector_open { return; }
        if self.cursor == 0 {
            if self.role_index > 0 {
                self.flush_current_fields();
                self.role_index -= 1;
                if self.active_panel == SettingsPanel::Provider {
                    self.load_role_fields();
                } else {
                    self.rebuild_fields_for_panel();
                }
            }
            return;
        }
        let fo = self.field_offset();
        let is_provider = fo == F_PROVIDER && self.active_panel == SettingsPanel::Provider;
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
        self.error = None;
        if self.selector_open { return; }
        if self.cursor == 0 {
            if self.role_index < self.role_options.len() - 1 {
                self.flush_current_fields();
                self.role_index += 1;
                if self.active_panel == SettingsPanel::Provider {
                    self.load_role_fields();
                } else {
                    self.rebuild_fields_for_panel();
                }
            }
            return;
        }
        let fo = self.field_offset();
        let is_provider = fo == F_PROVIDER && self.active_panel == SettingsPanel::Provider;
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

    fn selector_option_count(&self) -> usize {
        let fo = self.field_offset();
        match &self.fields[fo] {
            SettingsField::Selector { options, .. } => options.len(),
            _ => 0,
        }
    }

    fn selector_value(&self) -> String {
        let fo = self.field_offset();
        match &self.fields[fo] {
            SettingsField::Selector { options, index, .. } => options.get(*index).cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn selector_set_value(&mut self, value: String) {
        let fo = self.field_offset();
        if let SettingsField::Selector { options, index, .. } = &mut self.fields[fo] {
            if let Some(pos) = options.iter().position(|o| o == &value) {
                *index = pos;
            }
        }
    }

    pub fn selector_label_at(&self, filtered_index: usize) -> String {
        let real_index = self.selector_filtered_indices[filtered_index];
        let fo = self.field_offset();
        match &self.fields[fo] {
            SettingsField::Selector { labels, .. } => {
                labels.get(real_index).cloned().unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    pub fn type_char(&mut self, c: char) {
        self.error = None;
        if self.selector_open {
            self.selector_filter.push(c);
            self.rebuild_filtered_indices();
            return;
        }
        if self.cursor == 0 { return; }
        let fo = self.field_offset();
        if let SettingsField::Text { value, .. } = &mut self.fields[fo] {
            value.push(c);
            self.saved = false;
        }
    }

    pub fn backspace(&mut self) {
        self.error = None;
        if self.selector_open {
            self.selector_filter.pop();
            self.rebuild_filtered_indices();
            return;
        }
        if self.cursor == 0 { return; }
        let fo = self.field_offset();
        if let SettingsField::Text { value, .. } = &mut self.fields[fo] {
            value.pop();
            self.saved = false;
        }
    }

    pub fn open_selector(&mut self) {
        let fo = self.field_offset();
        if !matches!(self.fields[fo].kind(), FieldKind::Selector) { return; }
        self.selector_open = true;
        self.selector_cursor = match &self.fields[fo] {
            SettingsField::Selector { index, .. } => *index,
            _ => 0,
        };
        self.selector_filter.clear();
        self.rebuild_filtered_indices();
    }

    pub fn open_secret_edit(&mut self) {
        let fo = self.field_offset();
        if !matches!(self.fields[fo].kind(), FieldKind::Secret) { return; }
        self.secret_edit_open = true;
        self.secret_edit_buffer = match &self.fields[fo] {
            SettingsField::Secret { value, .. } => value.clone(),
            _ => String::new(),
        };
        self.secret_edit_cursor = self.secret_edit_buffer.len();
    }

    pub fn confirm_secret_edit(&mut self) {
        let fo = self.field_offset();
        if let SettingsField::Secret { value, .. } = &mut self.fields[fo] {
            *value = self.secret_edit_buffer.clone();
            self.saved = false;
            self.error = None;
        }
        self.secret_edit_open = false;
    }

    pub fn cancel_secret_edit(&mut self) {
        self.secret_edit_open = false;
    }

    pub fn secret_edit_home(&mut self) {
        self.secret_edit_cursor = 0;
    }

    pub fn secret_edit_end(&mut self) {
        self.secret_edit_cursor = self.secret_edit_buffer.len();
    }

    pub fn secret_edit_delete(&mut self) {
        if self.secret_edit_cursor < self.secret_edit_buffer.len() {
            self.secret_edit_buffer.remove(self.secret_edit_cursor);
        }
    }

    pub fn secret_edit_type_char(&mut self, c: char) {
        self.secret_edit_buffer.insert(self.secret_edit_cursor, c);
        self.secret_edit_cursor += 1;
    }

    pub fn secret_edit_backspace(&mut self) {
        if self.secret_edit_cursor > 0 {
            self.secret_edit_cursor -= 1;
            self.secret_edit_buffer.remove(self.secret_edit_cursor);
        }
    }

    pub fn secret_edit_left(&mut self) {
        if self.secret_edit_cursor > 0 {
            self.secret_edit_cursor -= 1;
        }
    }

    pub fn secret_edit_right(&mut self) {
        if self.secret_edit_cursor < self.secret_edit_buffer.len() {
            self.secret_edit_cursor += 1;
        }
    }

    pub fn confirm_selector(&mut self) {
        if !self.selector_open || self.selector_filtered_indices.is_empty() { return; }
        let fo = self.field_offset();
        let is_provider = fo == F_PROVIDER && self.active_panel == SettingsPanel::Provider;
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
                        let opt_lower = opt.to_lowercase();
                        let label_lower = labels.get(*i).map(|l| l.to_lowercase()).unwrap_or_default();
                        opt_lower.contains(&filter_lower) || label_lower.contains(&filter_lower)
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

    pub fn flush_fields_to_settings(&mut self) {
        match self.active_panel {
            SettingsPanel::Provider => self.flush_provider_fields(),
            SettingsPanel::Role => self.flush_behavior_fields(),
            SettingsPanel::Theme => self.flush_theme_fields(),
        }
    }

    fn flush_current_fields(&mut self) {
        self.flush_fields_to_settings();
    }

    fn flush_provider_fields(&mut self) {
        let role = &self.role_options[self.role_index];
        let provider_settings = self.provider_info.as_ref()
            .map(|info| info.settings.as_slice())
            .unwrap_or(&[]);
        let rs = gather_role_settings(&self.fields, provider_settings);
        self.all_settings.roles.insert(role.to_string(), rs);
    }

    fn flush_behavior_fields(&mut self) {
        let role = &self.role_options[self.role_index];
        if matches!(self.active_panel, SettingsPanel::Role) {
            let enabled = match &self.fields[0] {
                SettingsField::Selector { index, .. } => *index == 1,
                _ => false,
            };
            let beh = RoleBehaviorSettings {
                enabled: Some(enabled),
                ..Default::default()
            };
            self.all_settings.behaviors.insert(role.to_string(), beh);
        }
    }

    fn flush_theme_fields(&mut self) {
        if let Some(field) = self.fields.first() {
            if let SettingsField::Selector { options, index, .. } = field {
                if let Some(name) = options.get(*index) {
                    self.all_settings.theme = name.clone();
                }
            }
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
            _ => String::new(),
        };
        if !provider_id.is_empty() && !self.gateway_available {
            self.error = Some("Gateway not available — provider change limited".into());
        }

        let seq = self.async_op_seq.wrapping_add(1);
        self.async_op_seq = seq;
        self.pending_async = Some(PendingAsyncOp::ProviderChanged {
            seq,
            rs: gather_role_settings(&self.fields, self.provider_info.as_ref()
                .map(|info| info.settings.as_slice()).unwrap_or(&[])),
            providers: self.provider_metas.clone(),
            gateway_available: self.gateway_available,
        });
    }

    pub fn save(&mut self) -> Vec<crate::message::FrontendMessage> {
        let mut error_msgs = Vec::new();

        // Validate: at least one role must have a provider configured
        let any_configured = ROLES.iter().any(|role| {
            self.all_settings.roles.get(*role)
                .map(|rs| !rs.provider.is_empty())
                .unwrap_or(false)
        });

        if !any_configured {
            error_msgs.push("No provider configured".to_string());
        }

        // Validate: each configured role must have a model selected
        for role in ROLES {
            if let Some(rs) = self.all_settings.roles.get(*role) {
                if !rs.provider.is_empty() && rs.model.is_empty() {
                    error_msgs.push(format!("{}: model required", role_label(role)));
                }
            }
        }

        if !error_msgs.is_empty() {
            self.error = Some(error_msgs.join("\n"));
            return Vec::new();
        }

        self.flush_current_fields();
        self.saving = true;
        self.error = None;
        self.pending_async = Some(PendingAsyncOp::Save { all_settings: self.all_settings.clone() });

        Vec::new()
    }
}
