package providers

import (
	"context"
	"math"
)

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
	ID               string            `json:"id"`
	DefaultModel     string            `json:"default_model"`
	MaxTokensDefault int               `json:"max_tokens_default,omitempty"`
	Settings         []ProviderSetting `json:"settings,omitempty"`
	Features         ProviderFeatures  `json:"features"`
}

// WithMaxTokensSetting returns a copy of ProviderInfo with a standard max_tokens
// setting auto-injected. If the provider already declares max_tokens or
// max_completion_tokens in Settings, the existing entry is kept.
func (info *ProviderInfo) WithMaxTokensSetting() *ProviderInfo {
	if info.MaxTokensDefault <= 0 {
		return info
	}
	for _, s := range info.Settings {
		if s.Key == "max_tokens" || s.Key == "max_completion_tokens" {
			return info
		}
	}
	cp := *info
	cp.Settings = append(cp.Settings, ProviderSetting{
		Key:         "max_tokens",
		Label:       "Max Output Tokens",
		Type:        SettingNumber,
		Min:         fPtr(1),
		Default:     info.MaxTokensDefault,
		Scope:       ScopePerMode,
		Group:       "output",
		Description: "Maximum number of tokens in the response.",
	})
	return &cp
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
	ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error)
}

// ValidateOption customizes BaseValidateSettings behavior.
type ValidateOption func(*validateContext)

type validateContext struct {
	inactiveInThinkingExtra []string
	crossParamRules         []crossParamRule
}

type crossParamRule func(key string, val interface{}, settings map[string]interface{}) *SettingValidation

// InactiveInThinking marks additional keys as inactive during thinking mode.
func InactiveInThinking(keys ...string) ValidateOption {
	return func(ctx *validateContext) {
		ctx.inactiveInThinkingExtra = append(ctx.inactiveInThinkingExtra, keys...)
	}
}

// CrossParamRule adds a custom cross-parameter validation rule.
func CrossParamRule(fn func(key string, val interface{}, settings map[string]interface{}) *SettingValidation) ValidateOption {
	return func(ctx *validateContext) {
		ctx.crossParamRules = append(ctx.crossParamRules, fn)
	}
}

// BaseValidateSettings performs standard slider clamping and thinking-mode invalidation.
// Providers call this and then apply their own rules via ValidateOption.
func BaseValidateSettings(
	info *ProviderInfo,
	settings map[string]interface{},
	thinking *ThinkingConfig,
	opts ...ValidateOption,
) *ValidateSettingsResult {
	ctx := &validateContext{}
	for _, opt := range opts {
		opt(ctx)
	}

	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}
	isThinking := thinking != nil && thinking.Type == "enabled"

	// Standard keys that are inactive in thinking mode
	inThinking := map[string]bool{
		"temperature": true, "top_p": true, "presence_penalty": true, "frequency_penalty": true,
	}
	for _, k := range ctx.inactiveInThinkingExtra {
		inThinking[k] = true
	}

	// Keys that are inactive outside thinking mode
	thinkingOnly := map[string]bool{
		"reasoning_effort": true, "thinking_budget": true, "enable_thinking": true,
	}

	for _, s := range info.Settings {
		val := settings[s.Key]
		v := SettingValidation{Status: StatusActive}

		// Clamp slider values to [min, max]
		if s.Type == SettingSlider {
			num := toFloat(val)
			if num == 0 {
				num = floatDefault(s.Default, 0)
			}
			clamped := num
			if s.Min != nil {
				clamped = math.Max(clamped, *s.Min)
			}
			if s.Max != nil {
				clamped = math.Min(clamped, *s.Max)
			}
			if clamped != num {
				v.Value = clamped
			}
			val = clamped
		}

		// Active/inactive based on thinking mode
		if isThinking && inThinking[s.Key] {
			v.Status = StatusInactive
			v.Message = "Ignored in thinking mode"
		} else if !isThinking && thinkingOnly[s.Key] {
			v.Status = StatusInactive
			v.Message = "Only applies in thinking mode"
		}

		// Cross-parameter: logprobs requires top_logprobs > 0
		if s.Key == "top_logprobs" {
			logprobsEnabled, _ := settings["logprobs"].(bool)
			if logprobsEnabled {
				num := toFloat(val)
				if num <= 0 {
					v.Error = "Must be > 0 when logprobs is enabled"
					v.Value = float64(1)
				}
			}
		}

		// Custom cross-parameter rules
		for _, rule := range ctx.crossParamRules {
			if extra := rule(s.Key, val, settings); extra != nil {
				if extra.Status != "" {
					v.Status = extra.Status
				}
				if extra.Message != "" {
					v.Message = extra.Message
				}
				if extra.Error != "" {
					v.Error = extra.Error
				}
				if extra.Value != nil {
					v.Value = extra.Value
				}
			}
		}

		result.Settings[s.Key] = v
	}
	return result
}
