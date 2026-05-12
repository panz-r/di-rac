package providers

import (
	"context"
	"strings"
)

// XAIHandler handles xAI (Grok) API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with xAI-specific config:
//   - max_completion_tokens instead of max_tokens
//   - reasoning_effort for grok-3-mini models (low/high)
//   - reasoning_content in deltas intentionally skipped (Grok reasoning is not useful)
type XAIHandler struct {
	inner *openaiCompatHandler
}

func NewXAIHandler() *XAIHandler {
	const defaultModel = "grok-4"
	return &XAIHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.x.ai/v1",
			DefaultModel:        defaultModel,
			MaxCompletionTokens: true,
			Capabilities: &ProviderInfo{
				ID:           "xai",
				DefaultModel: defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsPromptCache:     false,
					SupportsStreaming:       true,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.01),
						Default:     0,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 2 = creative).",
						ValidRange:  "0 – 2",
					},
					{
						Key:         "top_p",
						Label:       "Top P",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     1.0,
						Group:       "sampling",
						Description: "Nucleus sampling threshold.",
						ValidRange:  "0 – 1",
					},
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "low", Label: "Low"},
							{Value: "high", Label: "High"},
						},
						Group:       "reasoning",
						Description: "Controls reasoning depth for grok-3-mini models. Only applies in thinking mode.",
					},
					{
						Key:        "presence_penalty",
						Label:      "Presence Penalty",
						Type:       SettingSlider,
						Min:        fPtr(-2),
						Max:        fPtr(2),
						Step:       fPtr(0.1),
						Group:      "sampling",
						ValidRange: "-2 – 2",
					},
					{
						Key:        "frequency_penalty",
						Label:      "Frequency Penalty",
						Type:       SettingSlider,
						Min:        fPtr(-2),
						Max:        fPtr(2),
						Step:       fPtr(0.1),
						Group:      "sampling",
						ValidRange: "-2 – 2",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				model, _ := result["model"].(string)
				// reasoning_effort for grok-3-mini models (only "low" or "high")
				if strings.Contains(model, "3-mini") {
					if effort := req.SettingString("reasoning_effort"); effort == "low" || effort == "high" {
						result["reasoning_effort"] = effort
					}
				}
				if req.SettingIsNull("temperature") {
					delete(result, "temperature")
				} else {
					result["temperature"] = req.SettingFloat("temperature")
				}
				if req.SettingIsNull("top_p") {
					delete(result, "top_p")
				} else {
					result["top_p"] = req.SettingFloat("top_p")
				}
				if req.SettingIsNull("presence_penalty") {
					delete(result, "presence_penalty")
				} else {
					result["presence_penalty"] = req.SettingFloat("presence_penalty")
				}
				if req.SettingIsNull("frequency_penalty") {
					delete(result, "frequency_penalty")
				} else {
					result["frequency_penalty"] = req.SettingFloat("frequency_penalty")
				}
			},
		}),
	}
}

func (h *XAIHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

// Stream wraps the inner handler to suppress reasoning_content thinking deltas.
// xAI's Grok reasoning output only displays "thinking" without useful content.
func (h *XAIHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, func(chunk StreamChunk) error {
		// Drop thinking deltas — Grok reasoning is not useful
		if chunk.Type == "delta" && chunk.Thinking != "" {
			return nil
		}
		return callback(chunk)
	})
}

func (h *XAIHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *XAIHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.inner.config.BaseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", cfg.APIKey)
}

func (h *XAIHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.inner.Capabilities(), settings, thinking)
}

var _ Handler = (*XAIHandler)(nil)
var _ CapableHandler = (*XAIHandler)(nil)
var _ SettingsValidator = (*XAIHandler)(nil)
var _ ModelLister = (*XAIHandler)(nil)
