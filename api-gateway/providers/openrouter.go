package providers

import (
	"context"
	"math"
	"strings"
)

// OpenRouterHandler handles OpenRouter API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with OpenRouter-specific config:
//   - Custom headers (HTTP-Referer, X-Title) for app attribution
//   - response_format support (json_object, json_schema)
//   - Provider routing and service tier selection
//   - Standard OpenAI sampling parameters from settings map
//   - Default model: anthropic/claude-sonnet-4.5
type OpenRouterHandler struct {
	inner *openaiCompatHandler
}

func NewOpenRouterHandler() *OpenRouterHandler {
	return &OpenRouterHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://openrouter.ai/api/v1",
			DefaultModel: "anthropic/claude-sonnet-4.5",
			ExtraHeaders: map[string]string{
				"HTTP-Referer": "https://dirac.run",
				"X-Title":      "Dirac",
			},
			Capabilities: &ProviderInfo{
				ID:           "openrouter",
				DefaultModel: "anthropic/claude-sonnet-4.5",
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsStreaming:       true,
					SupportsPromptCache:     true,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.01),
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
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "sampling",
						Description: "Custom stop sequences (comma-separated, max 4).",
					},
					{
						Key:         "logprobs",
						Label:       "Log Probabilities",
						Type:        SettingToggle,
						Group:       "sampling",
						Description: "Return log probabilities of output tokens.",
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
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "high", Label: "High"},
							{Value: "medium", Label: "Medium"},
							{Value: "low", Label: "Low"},
						},
						Group:       "reasoning",
						Description: "Controls reasoning depth for supported models. Only applies in thinking mode.",
					},
					{
						Key:   "response_format",
						Label: "Response Format",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "json_object", Label: "JSON"},
							{Value: "json_schema", Label: "JSON Schema"},
						},
						Description: "Force structured output format.",
					},
					{
						Key:   "provider",
						Label: "Provider",
						Type:  SettingSelect,
						Group: "routing",
						Options: []SelectOption{
							{Value: "", Label: "Auto"},
							{Value: "openai", Label: "OpenAI"},
							{Value: "anthropic", Label: "Anthropic"},
							{Value: "google", Label: "Google"},
							{Value: "mistral", Label: "Mistral"},
							{Value: "meta", Label: "Meta"},
							{Value: "deepseek", Label: "DeepSeek"},
						},
						Description: "Preferred upstream provider for model routing.",
					},
					{
						Key:   "service_tier",
						Label: "Service Tier",
						Type:  SettingSelect,
						Group: "routing",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "flex", Label: "Flex"},
						},
						Description: "Service tier (flex is cheaper but may queue).",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				isThinking := req.Thinking != nil && req.Thinking.Type == "enabled"

				if isThinking {
					effort := req.SettingString("reasoning_effort")
					if effort == "" && req.Thinking.ReasoningEffort != "" {
						effort = req.Thinking.ReasoningEffort
					}
					if effort != "" {
						result["reasoning_effort"] = effort
					}
					delete(result, "temperature")
					delete(result, "top_p")
				} else {
					result["temperature"] = req.SettingFloat("temperature")
					tp := req.SettingFloat("top_p")
					if tp == 0 {
						tp = 1.0
					}
					result["top_p"] = tp
				}

				// Presence/frequency penalties with typed field fallback
				pp := req.SettingFloat("presence_penalty")
				if pp == 0 {
					pp = req.PresencePenalty
				}
				if pp != 0 {
					result["presence_penalty"] = pp
				}
				fp := req.SettingFloat("frequency_penalty")
				if fp == 0 {
					fp = req.FrequencyPenalty
				}
				if fp != 0 {
					result["frequency_penalty"] = fp
				}

				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = strings.Split(stop, ",")
				}

				// Logprobs with typed field fallback
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

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				// OpenRouter-specific: provider routing
				if p := req.SettingString("provider"); p != "" {
					result["provider"] = map[string]string{"name": p}
				}

				// OpenRouter-specific: service tier
				if st := req.SettingString("service_tier"); st != "" {
					result["service_tier"] = st
				}
			},
		}),
	}
}

func (h *OpenRouterHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *OpenRouterHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *OpenRouterHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *OpenRouterHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}
	isThinking := thinking != nil && thinking.Type == "enabled"

	for _, s := range h.inner.Capabilities().Settings {
		val := settings[s.Key]
		v := SettingValidation{Status: StatusActive}

		// Clamp slider values to [min, max]
		if s.Type == SettingSlider {
			num := toFloat(val)
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

		// response_format: json_object or json_schema
		if s.Key == "response_format" {
			if rf, ok := val.(string); ok && rf != "" && rf != "json_object" && rf != "json_schema" {
				v.Error = "Must be 'json_object', 'json_schema', or empty"
				v.Value = ""
			}
		}

		// service_tier: only "flex" or empty
		if s.Key == "service_tier" {
			if st, ok := val.(string); ok && st != "" && st != "flex" {
				v.Error = "Must be 'flex' or empty"
				v.Value = ""
			}
		}

		// Cross-parameter: logprobs requires top_logprobs > 0
		if s.Key == "top_logprobs" && toFloat(settings["logprobs"]) != 0 {
			num := toFloat(val)
			if num <= 0 {
				v.Error = "Must be > 0 when logprobs is enabled"
				v.Value = float64(1)
			}
		}

		// stop: max 4 sequences
		if s.Key == "stop" {
			if stop, ok := val.(string); ok && stop != "" {
				seqs := strings.Split(stop, ",")
				if len(seqs) > 4 {
					v.Error = "Max 4 stop sequences"
					v.Value = strings.Join(seqs[:4], ",")
				}
			}
		}

		result.Settings[s.Key] = v
	}
	return result
}

var _ SettingsValidator = (*OpenRouterHandler)(nil)
