package providers

// SettingType determines the UI widget for a provider setting.
type SettingType string

const (
	SettingToggle SettingType = "toggle" // checkbox
	SettingSlider SettingType = "slider" // number with min/max/step
	SettingSelect SettingType = "select" // dropdown
	SettingText   SettingType = "text"   // text input
)

// ProviderSetting describes a single configurable parameter.
type ProviderSetting struct {
	Key         string         `json:"key"`
	Label       string         `json:"label"`
	Type        SettingType    `json:"type"`
	Default     interface{}    `json:"default,omitempty"`
	Min         *float64       `json:"min,omitempty"`
	Max         *float64       `json:"max,omitempty"`
	Step        *float64       `json:"step,omitempty"`
	Options     []SelectOption `json:"options,omitempty"`
	Description string         `json:"description,omitempty"`
	Group       string         `json:"group,omitempty"`
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

// CapableHandler is an optional interface for providers that expose capabilities.
// Handlers that don't implement it return nil from Registry.GetCapabilities.
type CapableHandler interface {
	Handler
	Capabilities() *ProviderInfo
}
