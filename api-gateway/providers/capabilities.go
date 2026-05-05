package providers

import "context"

// SettingType determines the UI widget for a provider setting.
type SettingType string

const (
	SettingToggle SettingType = "toggle" // checkbox
	SettingSlider SettingType = "slider" // number with min/max/step
	SettingSelect SettingType = "select" // dropdown
	SettingText   SettingType = "text"   // text input
	SettingNumber SettingType = "number" // integer input
)

// SettingScope determines whether a setting has one value or one per act/plan mode.
type SettingScope string

const (
	ScopeGlobal  SettingScope = "global"   // one value shared across modes
	ScopePerMode SettingScope = "per-mode" // separate value for act and plan
)

// ProviderSetting describes a single configurable parameter.
type ProviderSetting struct {
	Key         string         `json:"key"`
	Label       string         `json:"label"`
	Type        SettingType    `json:"type"`
	Scope       SettingScope   `json:"scope,omitempty"`
	Default     interface{}    `json:"default,omitempty"`
	Min         *float64       `json:"min,omitempty"`
	Max         *float64       `json:"max,omitempty"`
	Step        *float64       `json:"step,omitempty"`
	Options     []SelectOption `json:"options,omitempty"`
	Description string         `json:"description,omitempty"`
	Group       string         `json:"group,omitempty"`
	ValidRange  string         `json:"valid_range,omitempty"` // Human-readable constraint, e.g. "> 0 and ≤ 1"
}

// SelectOption is a value-label pair for select-type settings.
type SelectOption struct {
	Value string `json:"value"`
	Label string `json:"label,omitempty"`
}

// ProviderFeatures declares boolean capabilities.
type ProviderFeatures struct {
	SupportsThinking        bool `json:"supports_thinking"`
	SupportsReasoningEffort bool `json:"supports_reasoning_effort"`
	SupportsTools           bool `json:"supports_tools"`
	SupportsImages          bool `json:"supports_images"`
	SupportsPromptCache     bool `json:"supports_prompt_cache"`
	SupportsStreaming       bool `json:"supports_streaming"`
}

// ProviderInfo is the full capability descriptor for a provider.
type ProviderInfo struct {
	ID           string            `json:"id"`
	DefaultModel string            `json:"default_model"`
	Settings     []ProviderSetting `json:"settings,omitempty"`
	Features     ProviderFeatures  `json:"features"`
}

// SettingStatus describes whether a setting is currently in effect.
type SettingStatus string

const (
	StatusActive   SettingStatus = "active"   // setting is applied
	StatusInactive SettingStatus = "inactive" // setting is ignored (e.g., temperature in thinking mode)
)

// SettingValidation describes the validation result for a single setting.
type SettingValidation struct {
	Status  SettingStatus `json:"status"`            // "active" or "inactive"
	Value   interface{}   `json:"value,omitempty"`   // corrected/sanitized value (omitted if unchanged)
	Error   string        `json:"error,omitempty"`   // per-parameter error message
	Message string        `json:"message,omitempty"` // user-facing info about why it's inactive
}

// ValidateSettingsResult is the response from a validate-parameters query.
type ValidateSettingsResult struct {
	Settings map[string]SettingValidation `json:"settings"`
	Errors   []string                     `json:"errors,omitempty"` // cross-parameter errors
}

// ModelEntry describes a model available from a provider, used for model discovery.
type ModelEntry struct {
	ID                  string  `json:"id"`
	Name                string  `json:"name,omitempty"`
	Description         string  `json:"description,omitempty"`
	ContextWindow       int     `json:"context_window,omitempty"`
	MaxTokens           int     `json:"max_tokens,omitempty"`
	SupportsImages      bool    `json:"supports_images,omitempty"`
	SupportsPromptCache bool    `json:"supports_prompt_cache,omitempty"`
	InputPrice          float64 `json:"input_price,omitempty"`            // per million tokens
	OutputPrice         float64 `json:"output_price,omitempty"`           // per million tokens
	CacheWritesPrice    float64 `json:"cache_writes_price,omitempty"`
	CacheReadsPrice     float64 `json:"cache_reads_price,omitempty"`
	SupportsThinking    bool    `json:"supports_thinking,omitempty"`
	ThinkingMaxBudget   int     `json:"thinking_max_budget,omitempty"`
}

// CapableHandler is an optional interface for providers that expose capabilities.
// Handlers that don't implement it return nil from Registry.GetCapabilities.
type CapableHandler interface {
	Handler
	Capabilities() *ProviderInfo
}

// SettingsValidator is an optional interface for providers that validate settings.
type SettingsValidator interface {
	CapableHandler
	ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult
}

// ModelLister is an optional interface for providers that can discover available models.
// Providers that don't implement it return nil from Registry.ListModels.
type ModelLister interface {
	CapableHandler
	ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error)
}
