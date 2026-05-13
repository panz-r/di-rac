package providers

import (
	"context"
	"math"
)

// DeepSeekHandler handles DeepSeek API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with DeepSeek-specific config:
//   - max_completion_tokens instead of max_tokens
//   - strict: true on all tool function definitions
//   - R1 format: reasoning_content on assistant messages, coerced non-null
//   - Thinking mode: omit temperature/top_p, enable thinking parameter
//   - Default model: deepseek-chat
type DeepSeekHandler struct {
	inner *openaiCompatHandler
}

func fPtr(v float64) *float64 { return &v }

func NewDeepSeekHandler() *DeepSeekHandler {
	const defaultModel = "deepseek-v4-pro"
	return &DeepSeekHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.deepseek.com/v1",
			DefaultModel:        defaultModel,
			MaxCompletionTokens: true,
			StrictTools:         true,
			Capabilities: &ProviderInfo{
				ID:           "deepseek",
				MaxTokensDefault: 16384,
				DefaultModel: defaultModel,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          false,
					SupportsPromptCache:     true,
					SupportsStreaming:       true,
				},
				Settings: []ProviderSetting{
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "high", Label: "High"},
							{Value: "max", Label: "Max"},
						},
						Group:       "reasoning",
						Description: "Controls reasoning depth for thinking mode",
					},
					{
						Key:         "logprobs",
						Label:       "Log Probabilities",
						Type:        SettingToggle,
						Group:       "sampling",
						Description: "Return log probabilities of output tokens",
					},
					{
						Key:        "top_logprobs",
						Label:      "Top Logprobs",
						Type:       SettingSlider,
						Min:        fPtr(0),
						Max:        fPtr(20),
						Step:       fPtr(1),
						Group:      "sampling",
						ValidRange: "0 – 20",
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
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Default:     1.0,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 2 = creative). Ignored in thinking mode.",
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
						Description: "Nucleus sampling threshold. Ignored in thinking mode.",
						ValidRange:  "0 – 1",
					},
				},
			},
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				messages = openaiAddReasoningContent(messages, req)
				messages = coerceAssistantMessages(messages)
				return messages
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				if req.Thinking != nil && req.Thinking.Type == "enabled" {
					// DeepSeek thinking mode
					result["thinking"] = map[string]interface{}{
						"type": req.Thinking.Type,
					}
					// Read reasoning_effort from generic settings map, fallback to Thinking.ReasoningEffort, default "high"
					reasoningEffort := req.SettingString("reasoning_effort")
					if reasoningEffort == "" && req.Thinking.ReasoningEffort != "" {
						reasoningEffort = req.Thinking.ReasoningEffort
					}
					if reasoningEffort == "" {
						reasoningEffort = "high"
					}
					result["reasoning_effort"] = reasoningEffort
					// Thinking mode ignores temperature/top_p
					delete(result, "temperature")
					delete(result, "top_p")
				} else {
					// Non-thinking mode: apply temperature and top_p from settings
					req.ApplySettingFloat(result, "temperature")
					if req.SettingIsNull("top_p") {
						delete(result, "top_p")
					} else {
						topP := req.SettingFloat("top_p")
						if topP == 0 {
							topP = 1.0
						}
						result["top_p"] = topP
					}
				}
				// Read sampling settings from generic map, fallback to typed fields
				logprobs := req.SettingBool("logprobs")
				if !logprobs {
					logprobs = req.Logprobs
				}
				if logprobs {
					result["logprobs"] = true
					topLogprobs := int(req.SettingFloat("top_logprobs"))
					if topLogprobs == 0 {
						topLogprobs = req.TopLogprobs
					}
					if topLogprobs > 0 {
						result["top_logprobs"] = topLogprobs
					}
				}
				presencePenalty := req.SettingFloat("presence_penalty")
				if presencePenalty == 0 {
					presencePenalty = req.PresencePenalty
				}
				if presencePenalty != 0 {
					result["presence_penalty"] = presencePenalty
				}
				frequencyPenalty := req.SettingFloat("frequency_penalty")
				if frequencyPenalty == 0 {
					frequencyPenalty = req.FrequencyPenalty
				}
				if frequencyPenalty != 0 {
					result["frequency_penalty"] = frequencyPenalty
				}
			},
		}),
	}
}

func (h *DeepSeekHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *DeepSeekHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *DeepSeekHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *DeepSeekHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}
	isThinking := thinking != nil && thinking.Type == "enabled"

	for _, s := range h.inner.Capabilities().Settings {
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
		if isThinking {
			switch s.Key {
			case "temperature", "top_p", "presence_penalty", "frequency_penalty":
				v.Status = StatusInactive
				v.Message = "Ignored in thinking mode"
			}
		} else if s.Key == "reasoning_effort" {
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

		result.Settings[s.Key] = v
	}
	return result
}

var _ SettingsValidator = (*DeepSeekHandler)(nil)

// coerceAssistantMessages ensures all assistant messages have non-null content
// and non-null reasoning_content, as required by DeepSeek.
// Returns a copy to avoid mutating the input slice.
func coerceAssistantMessages(messages []map[string]interface{}) []map[string]interface{} {
	result := make([]map[string]interface{}, len(messages))
	for i, msg := range messages {
		m := make(map[string]interface{}, len(msg)+2)
		for k, v := range msg {
			m[k] = v
		}
		if role, ok := m["role"].(string); ok && role == "assistant" {
			if m["content"] == nil {
				m["content"] = ""
			}
			if _, ok := m["reasoning_content"]; !ok {
				m["reasoning_content"] = ""
			}
		}
		result[i] = m
	}
	return result
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *DeepSeekHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ ModelLister = (*DeepSeekHandler)(nil)
