package providers

import (
	"context"
	"math"
	"strings"
)

// ZAIHandler handles Zhipu AI (ZAI) API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with ZAI-specific config:
//   - Custom headers (HTTP-Referer, X-Title)
//   - thinking: { type: "enabled" } (no budget_tokens)
//   - tool_stream: true for streaming tool calls
//   - reasoning_content in streaming deltas
//   - Dynamic base URL (international, china, coding-plan)
//   - Default model: glm-5
type ZAIHandler struct {
	inner *openaiCompatHandler
}

func NewZAIHandler() *ZAIHandler {
	return &ZAIHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.z.ai/api/paas/v4",
			DefaultModel:        "glm-5",
			ContentArraySupport: true,
			ExtraHeaders: map[string]string{
				"HTTP-Referer": "https://dirac.run",
				"X-Title":      "Dirac",
			},
			FinishReasonMap: func(reason string) string {
				if reason == "model_context_window_exceeded" {
					return "length"
				}
				return reason
			},
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				messages = openaiAddReasoningContent(messages, req)
				return coerceAssistantMessages(messages)
			},
			Capabilities: &ProviderInfo{
				ID:           "zai",
				DefaultModel: "glm-5",
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
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     1.0,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 1 = creative). Ignored in thinking mode.",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "top_p",
						Label:       "Top P",
						Type:        SettingSlider,
						Min:         fPtr(0.01),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     0.95,
						Group:       "sampling",
						Description: "Nucleus sampling threshold. Ignored in thinking mode.",
						ValidRange:  "0.01 – 1",
					},
					{
						Key:         "do_sample",
						Label:       "Enable Sampling",
						Type:        SettingToggle,
						Default:     true,
						Group:       "sampling",
						Description: "Enable sampling (default: true). Ignored in thinking mode.",
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
							{Value: "low", Label: "Low"},
						},
						Group:       "reasoning",
						Description: "Controls reasoning depth (GLM-4.5+ only). Only applies in thinking mode.",
					},
					{
						Key:         "clear_thinking",
						Label:       "Clear Thinking",
						Type:        SettingToggle,
						Default:     true,
						Group:       "reasoning",
						Description: "Clear reasoning_content from previous turns (default: true). Only applies in thinking mode.",
					},
					{
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "output",
						Description: "Custom stop sequence (max 1 for ZAI).",
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
						Key:   "api_line",
						Label: "API Line",
						Type:  SettingSelect,
						Group: "provider",
						Options: []SelectOption{
							{Value: "international", Label: "International"},
							{Value: "coding-plan", Label: "Coding Plan"},
							{Value: "china", Label: "China"},
						},
						Default:     "international",
						Description: "ZAI API endpoint region.",
					},
					{
						Key:         "user_id",
						Label:       "User ID",
						Type:        SettingText,
						Group:       "provider",
						Description: "Unique end-user ID (6–128 characters).",
					},
					{
						Key:         "request_id",
						Label:       "Request ID",
						Type:        SettingText,
						Group:       "provider",
						Description: "Unique request ID (optional).",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				isThinking := req.Thinking != nil && req.Thinking.Type == "enabled"

				if isThinking {
					thinkingConfig := map[string]interface{}{"type": "enabled"}
					if !req.SettingBool("clear_thinking") {
						thinkingConfig["clear_thinking"] = false
					}
					result["thinking"] = thinkingConfig
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
						tp = 0.95
					}
					result["top_p"] = tp
				}

				if !req.SettingBool("do_sample") {
					result["do_sample"] = false
				}

				if len(req.Tools) > 0 {
					result["tool_stream"] = true
				}

				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = []string{stop}
				}

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

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

				if userID := req.SettingString("user_id"); userID != "" {
					result["user_id"] = userID
				}
				if requestID := req.SettingString("request_id"); requestID != "" {
					result["request_id"] = requestID
				}
			},
		}),
	}
}

func (h *ZAIHandler) resolveBaseURL(req *Request) string {
	if req.Provider.BaseURL != "" {
		return req.Provider.BaseURL
	}
	switch req.SettingString("api_line") {
	case "china":
		return "https://open.bigmodel.cn/api/paas/v4"
	case "coding-plan":
		return "https://api.z.ai/api/coding/paas/v4"
	default:
		return "https://api.z.ai/api/paas/v4"
	}
}

func (h *ZAIHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	h.inner.config.BaseURL = h.resolveBaseURL(req)
	return h.inner.Send(ctx, req)
}

func (h *ZAIHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	h.inner.config.BaseURL = h.resolveBaseURL(req)
	return h.inner.Stream(ctx, req, callback)
}

func (h *ZAIHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *ZAIHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
			case "temperature", "top_p", "do_sample":
				v.Status = StatusInactive
				v.Message = "Ignored in thinking mode"
			}
		} else {
			switch s.Key {
			case "reasoning_effort", "clear_thinking":
				v.Status = StatusInactive
				v.Message = "Only applies in thinking mode"
			}
		}

		// ZAI: stop is limited to 1 sequence
		if s.Key == "stop" {
			if stopSeq, ok := val.(string); ok && strings.Contains(stopSeq, ",") {
				v.Error = "ZAI supports only 1 stop sequence"
				v.Value = strings.Split(stopSeq, ",")[0]
			}
		}

		// response_format: only "json_object" is valid
		if s.Key == "response_format" {
			if rf, ok := val.(string); ok && rf != "" && rf != "json_object" {
				v.Error = "Must be 'json_object' or empty"
				v.Value = ""
			}
		}

		// reasoning_effort: only "high" or "low"
		if s.Key == "reasoning_effort" {
			if effort, ok := val.(string); ok && effort != "" && effort != "high" && effort != "low" {
				v.Error = "Must be 'high' or 'low'"
				v.Value = "high"
			}
		}

		// user_id: 6–128 characters
		if s.Key == "user_id" {
			if uid, ok := val.(string); ok && len(uid) > 0 && (len(uid) < 6 || len(uid) > 128) {
				v.Error = "Must be 6–128 characters"
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

		result.Settings[s.Key] = v
	}
	return result
}

var _ SettingsValidator = (*ZAIHandler)(nil)

// ListModels delegates to the shared openaiCompatHandler model discovery.
// Uses cfg.BaseURL if set, otherwise the default international endpoint.
func (h *ZAIHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	if cfg.BaseURL == "" {
		cfg.BaseURL = "https://api.z.ai/api/paas/v4"
	}
	return h.inner.ListModels(ctx, cfg)
}

var _ ModelLister = (*ZAIHandler)(nil)
