package providers

import (
	"context"
	"math"
)

// BytePlusCodingPlanHandler handles BytePlus Coding Plan (subscription) API requests.
// BytePlus Coding Plan is a subscription-based coding assistant with:
//   - OpenAI-compatible /api/coding/v3/chat/completions endpoint
//   - Deep reasoning (thinking) support
//   - Base URL: https://ark.ap-southeast.bytepluses.com/api/coding/v3
//   - No model listing — user types model ID directly
//   - No reasoning_effort, logprobs, response_format, or context_caching
type BytePlusCodingPlanHandler struct {
	inner *openaiCompatHandler
}

func NewBytePlusCodingPlanHandler() *BytePlusCodingPlanHandler {
	const defaultModel = "ark-code-latest"
	return &BytePlusCodingPlanHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
			DefaultModel:        defaultModel,
			MaxCompletionTokens: true,
			Capabilities: &ProviderInfo{
				ID:           "byteplus_coding_plan",
				DefaultModel: defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: false,
					SupportsTools:           true,
					SupportsImages:          false,
					SupportsStreaming:       true,
				},
				Settings: []ProviderSetting{
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
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "sampling",
						Description: "Custom stop sequences (comma-separated, max 4).",
					},
					{
						Key:         "seed",
						Label:       "Seed",
						Type:        SettingNumber,
						Min:         fPtr(0),
						Group:       "sampling",
						Description: "Random seed for deterministic outputs.",
					},
					{
						Key:         "user",
						Label:       "User ID",
						Type:        SettingText,
						Group:       "metadata",
						Description: "End-user identifier for abuse monitoring.",
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
					delete(result, "presence_penalty")
					delete(result, "frequency_penalty")
				} else {
					if temp := req.SettingFloat("temperature"); temp != 0 {
						result["temperature"] = temp
					}
					if topP := req.SettingFloat("top_p"); topP != 0 {
						result["top_p"] = topP
					}
					if pp := req.SettingFloat("presence_penalty"); pp != 0 {
						result["presence_penalty"] = pp
					}
					if fp := req.SettingFloat("frequency_penalty"); fp != 0 {
						result["frequency_penalty"] = fp
					}
				}

				if stop := req.SettingString("stop"); stop != "" {
					if seqs := splitStopSequences(stop); seqs != nil {
					result["stop"] = seqs
				}
				}
				if seed := req.SettingInt("seed"); seed > 0 {
					result["seed"] = seed
				}
				if user := req.SettingString("user"); user != "" {
					result["user"] = user
				}
			},
		}),
	}
}

func (h *BytePlusCodingPlanHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *BytePlusCodingPlanHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, NewThinkTagStream(callback))
}

func (h *BytePlusCodingPlanHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *BytePlusCodingPlanHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
		}

		if s.Key == "thinking" {
			if tv, ok := val.(string); ok && tv != "" {
				switch tv {
				case "enabled", "disabled", "auto":
				default:
					v.Error = "Must be 'enabled', 'disabled', or 'auto'"
					v.Value = ""
				}
			}
		}

		result.Settings[s.Key] = v
	}
	return result
}

var _ Handler = (*BytePlusCodingPlanHandler)(nil)
var _ CapableHandler = (*BytePlusCodingPlanHandler)(nil)
var _ SettingsValidator = (*BytePlusCodingPlanHandler)(nil)
