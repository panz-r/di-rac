package providers

import (
	"context"
	"math"
	"strings"
)

// OpenAICodexHandler handles OpenAI Codex API requests via the Responses API.
// Codex is a subscription-based coding agent that requires ChatGPT OAuth.
type OpenAICodexHandler struct {
	inner *responsesAPIHandler
}

func NewOpenAICodexHandler() *OpenAICodexHandler {
	const defaultModel = "gpt-5.3-codex"
	return &OpenAICodexHandler{
		inner: newResponsesAPIHandler(ResponsesAPIConfig{
			BaseURL:      "https://chatgpt.com/backend-api/codex",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:           "openai_codex",
				MaxTokensDefault: 16384,
				DefaultModel: defaultModel,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsStreaming:       true,
					SupportsPromptCache:     false,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Default:     0.7,
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
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Group:       "reasoning",
						Description: "Controls reasoning depth. Only applies in thinking mode.",
					},
					{
						Key:         "max_output_tokens",
						Label:       "Max Output Tokens",
						Type:        SettingNumber,
						Min:         fPtr(1),
						Group:       "sampling",
						Description: "Maximum tokens in the response.",
					},
					{
						Key:   "response_format",
						Label: "Response Format",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "json_object", Label: "JSON"},
						},
						Description: "Force JSON output format.",
					},
					{
						Key:   "verbosity",
						Label: "Verbosity",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Description: "Controls output verbosity.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Temperature
				if req.SettingIsNull("temperature") {
					delete(result, "temperature")
				} else if temp := req.SettingFloat("temperature"); temp > 0 {
					result["temperature"] = temp
				}

				// Top P
				if req.SettingIsNull("top_p") {
					delete(result, "top_p")
				} else if topP := req.SettingFloat("top_p"); topP > 0 {
					result["top_p"] = topP
				}

				// Response format
				if rf := req.SettingString("response_format"); rf != "" {
					result["text"] = map[string]interface{}{
						"format": map[string]string{"type": rf},
					}
				}

				// Verbosity
				if v := req.SettingString("verbosity"); v != "" {
					// Merge into text config
					text, _ := result["text"].(map[string]interface{})
					if text == nil {
						text = make(map[string]interface{})
					}
					text["verbosity"] = v
					result["text"] = text
				}

				// Reasoning effort from settings (if not already set by thinking config)
				if req.Thinking == nil {
					if effort := req.SettingString("reasoning_effort"); effort != "" {
						result["reasoning"] = map[string]interface{}{
							"effort":  effort,
							"summary": "auto",
						}
					}
				}
			},
		}),
	}
}

func (h *OpenAICodexHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *OpenAICodexHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *OpenAICodexHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels returns the static list of Codex models.
// The Codex backend (chatgpt.com/backend-api/codex) does not expose a /models
// endpoint for OAuth tokens, so we use a hardcoded list.
func (h *OpenAICodexHandler) ListModels(_ context.Context, _ ProviderConfig) ([]ModelEntry, error) {
	return staticCodexModels(), nil
}

func staticCodexModels() []ModelEntry {
	return []ModelEntry{
		{ID: "gpt-5.5", Name: "GPT-5.5", ContextWindow: 128000},
		{ID: "gpt-5.4", Name: "GPT-5.4", ContextWindow: 128000},
		{ID: "gpt-5.4-mini", Name: "GPT-5.4 Mini", ContextWindow: 128000},
		{ID: "gpt-5.3-codex", Name: "GPT-5.3 Codex", ContextWindow: 128000},
		{ID: "gpt-5.3-codex-spark", Name: "GPT-5.3 Codex Spark", ContextWindow: 128000},
		{ID: "gpt-5.2-codex", Name: "GPT-5.2 Codex", ContextWindow: 128000},
		{ID: "gpt-5.2", Name: "GPT-5.2", ContextWindow: 128000},
		{ID: "gpt-5.1-codex", Name: "GPT-5.1 Codex", ContextWindow: 128000},
		{ID: "gpt-5.1-codex-mini", Name: "GPT-5.1 Codex Mini", ContextWindow: 128000},
		{ID: "codex", Name: "Codex (Default)", ContextWindow: 32768},
	}
}

// ValidateSettings validates user-provided settings for the Codex provider.
func (h *OpenAICodexHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}

	for _, s := range h.inner.Capabilities().Settings {
		val := settings[s.Key]
		v := SettingValidation{Status: StatusActive}

		// Clamp slider values
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
		}

		// Validate enums
		switch s.Key {
		case "response_format":
			if rf, ok := val.(string); ok && rf != "" && rf != "json_object" {
				v.Error = "Must be 'json_object' or empty"
				v.Value = ""
			}
		case "verbosity":
			valid := map[string]bool{"": true, "low": true, "medium": true, "high": true}
			if vv, ok := val.(string); ok && !valid[vv] {
				v.Error = "Must be 'low', 'medium', 'high', or empty"
				v.Value = ""
			}
		case "reasoning_effort":
			valid := map[string]bool{"": true, "low": true, "medium": true, "high": true}
			if vv, ok := val.(string); ok && !valid[vv] {
				v.Error = "Must be 'low', 'medium', 'high', or empty"
				v.Value = ""
			}
		}

		result.Settings[s.Key] = v
	}
	return result
}

// stripCodexPrefix removes a provider prefix from the model name if present.
func stripCodexPrefix(model string) string {
	for _, prefix := range []string{"openai_codex/"} {
		if strings.HasPrefix(model, prefix) {
			return strings.TrimPrefix(model, prefix)
		}
	}
	return model
}

// Compile-time interface assertions.
var (
	_ Handler          = (*OpenAICodexHandler)(nil)
	_ CapableHandler   = (*OpenAICodexHandler)(nil)
	_ ModelLister      = (*OpenAICodexHandler)(nil)
	_ SettingsValidator = (*OpenAICodexHandler)(nil)
)
