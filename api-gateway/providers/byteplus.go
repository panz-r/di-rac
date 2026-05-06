package providers

import (
	"context"
	"math"
	"strings"
)

// BytePlusHandler handles BytePlus ModelArk API requests.
// BytePlus ModelArk is ByteDance's unified AI model platform with:
//   - OpenAI-compatible /v3/chat/completions endpoint
//   - Deep reasoning (thinking) support
//   - Tool calling
//   - Context caching
//   - Base URL: https://ark.ap-southeast.bytepluses.com/api/v3
type BytePlusHandler struct {
	inner *openaiCompatHandler
}

func NewBytePlusHandler() *BytePlusHandler {
	const defaultModel = "seed-2-0-lite-260228"
	return &BytePlusHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://ark.ap-southeast.bytepluses.com/api/v3",
			DefaultModel:        defaultModel,
			MaxCompletionTokens: true,
			Capabilities: &ProviderInfo{
				ID:           "byteplus",
				DefaultModel: defaultModel,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsPromptCache:     true,
					SupportsStreaming:       true,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     0.7,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 1 = creative). Ignored in thinking mode.",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "top_p",
						Label:       "Top P",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     0.9,
						Group:       "sampling",
						Description: "Nucleus sampling threshold. Ignored in thinking mode.",
						ValidRange:  "0 – 1",
					},
					{
						Key:   "thinking",
						Label: "Deep Reasoning",
						Type:  SettingSelect,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default (auto)"},
							{Value: "enabled", Label: "Enabled"},
							{Value: "disabled", Label: "Disabled"},
							{Value: "auto", Label: "Auto"},
						},
						Description: "Enable deep reasoning (thinking mode).",
					},
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "minimal", Label: "Minimal"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Description: "Controls reasoning depth. Only applies in thinking mode.",
					},
					{
						Key:         "presence_penalty",
						Label:       "Presence Penalty",
						Type:        SettingSlider,
						Min:         fPtr(-2),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Group:       "sampling",
						ValidRange:  "-2 – 2",
						Description: "Penalizes new tokens based on presence in context. Ignored in thinking mode.",
					},
					{
						Key:         "frequency_penalty",
						Label:       "Frequency Penalty",
						Type:        SettingSlider,
						Min:         fPtr(-2),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Group:       "sampling",
						ValidRange:  "-2 – 2",
						Description: "Penalizes repeated tokens. Ignored in thinking mode.",
					},
					{
						Key:         "logprobs",
						Label:       "Log Probabilities",
						Type:        SettingToggle,
						Group:       "sampling",
						Description: "Return log probabilities of output tokens.",
					},
					{
						Key:         "top_logprobs",
						Label:       "Top Logprobs",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(20),
						Step:        fPtr(1),
						Group:       "sampling",
						ValidRange:  "0 – 20",
						Description: "Number of top log probabilities to return.",
					},
					{
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "sampling",
						Description: "Custom stop sequences (comma-separated, max 4).",
					},
					{
						Key:   "response_format",
						Label: "Response Format",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "json_object", Label: "JSON Object"},
						},
						Description: "Force structured output format.",
					},
					{
						Key:         "context_caching",
						Label:       "Context Caching",
						Type:        SettingToggle,
						Group:       "byteplus",
						Description: "Enable prompt caching to reduce costs.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				thinkingVal := req.SettingString("thinking")
				if thinkingVal != "" {
					result["thinking"] = map[string]string{"type": thinkingVal}
				} else if req.Thinking != nil && req.Thinking.Type == "enabled" {
					result["thinking"] = map[string]string{"type": "enabled"}
				}

				thinkingActive := thinkingVal == "enabled" || thinkingVal == "auto" ||
					(thinkingVal == "" && req.Thinking != nil && req.Thinking.Type == "enabled")

				if thinkingActive {
					delete(result, "temperature")
					delete(result, "top_p")
					reasoningEffort := req.SettingString("reasoning_effort")
					if reasoningEffort == "" && req.Thinking != nil && req.Thinking.ReasoningEffort != "" {
						reasoningEffort = req.Thinking.ReasoningEffort
					}
					if reasoningEffort != "" {
						result["reasoning_effort"] = reasoningEffort
					}
				} else {
					if temp := req.SettingFloat("temperature"); temp != 0 {
						result["temperature"] = temp
					}
					if topP := req.SettingFloat("top_p"); topP != 0 {
						result["top_p"] = topP
					}
					delete(result, "reasoning_effort")
				}

				if pp := req.SettingFloat("presence_penalty"); pp != 0 {
					result["presence_penalty"] = pp
				}
				if fp := req.SettingFloat("frequency_penalty"); fp != 0 {
					result["frequency_penalty"] = fp
				}

				if logprobs := req.SettingBool("logprobs"); logprobs {
					result["logprobs"] = true
					if topLogprobs := int(req.SettingFloat("top_logprobs")); topLogprobs > 0 {
						result["top_logprobs"] = topLogprobs
					}
				}

				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = splitStopSequences(stop)
				}

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				if req.SettingBool("context_caching") {
					result["context_caching"] = true
				}
			},
		}),
	}
}

func splitStopSequences(s string) []string {
	var parts []string
	for _, p := range strings.Split(s, ",") {
		p = strings.TrimSpace(p)
		if p != "" {
			parts = append(parts, p)
		}
	}
	if len(parts) > 4 {
		parts = parts[:4]
	}
	return parts
}

func (h *BytePlusHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *BytePlusHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *BytePlusHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *BytePlusHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}

	thinkingActive := false
	if thinkingVal, ok := settings["thinking"].(string); ok {
		thinkingActive = thinkingVal == "enabled" || thinkingVal == "auto"
	}
	if !thinkingActive && thinking != nil && thinking.Type == "enabled" {
		thinkingActive = true
	}

	for _, s := range h.inner.Capabilities().Settings {
		val := settings[s.Key]
		v := SettingValidation{Status: StatusActive}

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

		if thinkingActive {
			switch s.Key {
			case "temperature", "top_p", "presence_penalty", "frequency_penalty":
				v.Status = StatusInactive
				v.Message = "Ignored in thinking mode"
			}
		} else if s.Key == "reasoning_effort" {
			v.Status = StatusInactive
			v.Message = "Only applies in thinking mode"
		}

		switch s.Key {
		case "thinking":
			if tv, ok := val.(string); ok && tv != "" {
				switch tv {
				case "enabled", "disabled", "auto":
				default:
					v.Error = "Must be 'enabled', 'disabled', or 'auto'"
					v.Value = ""
				}
			}
		case "reasoning_effort":
			if ev, ok := val.(string); ok && ev != "" {
				switch ev {
				case "minimal", "low", "medium", "high":
				default:
					v.Error = "Must be 'minimal', 'low', 'medium', or 'high'"
					v.Value = ""
				}
			}
		}

		if s.Key == "top_logprobs" {
			if logprobsEnabled, _ := settings["logprobs"].(bool); logprobsEnabled {
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

func (h *BytePlusHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ Handler = (*BytePlusHandler)(nil)
var _ CapableHandler = (*BytePlusHandler)(nil)
var _ SettingsValidator = (*BytePlusHandler)(nil)
var _ ModelLister = (*BytePlusHandler)(nil)
