package providers

import (
	"context"
	"strings"
)

// NebiusHandler handles Nebius Token Factory API requests.
// Nebius Token Factory is an OpenAI-compatible inference API.
//   - Base URL: https://api.tokenfactory.nebius.com/v1
//   - Model format: {org}/{model} (e.g., Qwen/Qwen2.5-32B-Instruct-fast)
//   - reasoning_content in messages for DeepSeek-R1
//   - reasoning_effort: low/medium/high
//   - max_completion_tokens supported (in addition to max_tokens)
//   - response_format: json_object or text (no json_schema)
//   - service_tier: auto/default/over-limit/flex/no-limit
type NebiusHandler struct {
	inner *openaiCompatHandler
}

func NewNebiusHandler() *NebiusHandler {
	const defaultModel = "Qwen/Qwen2.5-32B-Instruct-fast"
	return &NebiusHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.tokenfactory.nebius.com/v1",
			DefaultModel:        defaultModel,
			MaxCompletionTokens: true,
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				model := req.Provider.Model
				if req.ModelOverride != "" {
					model = req.ModelOverride
				}
				if strings.Contains(model, "DeepSeek-R1") {
					messages = openaiAddReasoningContent(messages, req)
				}
				return messages
			},
			Capabilities: &ProviderInfo{
				ID:               "nebius",
				DefaultModel:     defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
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
						Default:     1.0,
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
						Key:         "presence_penalty",
						Label:       "Presence Penalty",
						Type:        SettingSlider,
						Min:         fPtr(-2),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Group:       "sampling",
						ValidRange:  "-2 – 2",
						Description: "Penalizes new tokens based on presence in context.",
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
						Description: "Penalizes repeated tokens.",
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
						Group:       "output",
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
							{Value: "text", Label: "Text"},
						},
						Description: "Force structured output format.",
					},
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Description: "Controls reasoning depth for reasoning models.",
					},
					{
						Key:   "service_tier",
						Label: "Service Tier",
						Type:  SettingSelect,
						Group: "provider",
						Options: []SelectOption{
							{Value: "", Label: "Default (auto)"},
							{Value: "auto", Label: "Auto"},
							{Value: "default", Label: "Default"},
							{Value: "over-limit", Label: "Over Limit"},
							{Value: "flex", Label: "Flex"},
							{Value: "no-limit", Label: "No Limit"},
						},
						Description: "Service tier for priority/latency control.",
					},
					{
						Key:         "store",
						Label:       "Store Output",
						Type:        SettingToggle,
						Group:       "provider",
						Description: "Store outputs for model distillation.",
					},
					{
						Key:         "user",
						Label:       "User ID",
						Type:        SettingText,
						Group:       "provider",
						Description: "End-user identifier for abuse monitoring.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Temperature: override buildRequest default of 0
				result["temperature"] = req.SettingFloat("temperature")

				if topP := req.SettingFloat("top_p"); topP > 0 {
					result["top_p"] = topP
				}

				if pp := req.SettingFloat("presence_penalty"); pp != 0 {
					result["presence_penalty"] = pp
				}
				if fp := req.SettingFloat("frequency_penalty"); fp != 0 {
					result["frequency_penalty"] = fp
				}

				if seed := req.SettingInt("seed"); seed > 0 {
					result["seed"] = seed
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

				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = splitStopSequences(stop)
				}

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				if effort := req.SettingString("reasoning_effort"); effort != "" {
					result["reasoning_effort"] = effort
				} else if req.Thinking != nil && req.Thinking.ReasoningEffort != "" {
					result["reasoning_effort"] = req.Thinking.ReasoningEffort
				}

				if tier := req.SettingString("service_tier"); tier != "" {
					result["service_tier"] = tier
				}

				if req.SettingBool("store") {
					result["store"] = true
				}

				if user := req.SettingString("user"); user != "" {
					result["user"] = user
				}
			},
		}),
	}
}

func (h *NebiusHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *NebiusHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *NebiusHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *NebiusHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.inner.Capabilities(), settings, thinking,
		CrossParamRule(func(key string, val interface{}, allSettings map[string]interface{}) *SettingValidation {
			switch key {
			case "stop":
				if s, ok := val.(string); ok && s != "" {
					seqs := strings.Split(s, ",")
					if len(seqs) > 4 {
						return &SettingValidation{
							Error: "Maximum 4 stop sequences allowed",
						}
					}
				}
			case "response_format":
				valid := map[string]bool{"": true, "json_object": true, "text": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'json_object', 'text', or empty",
						Value: "",
					}
				}
			case "reasoning_effort":
				valid := map[string]bool{"": true, "low": true, "medium": true, "high": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'low', 'medium', 'high', or empty",
						Value: "",
					}
				}
			case "service_tier":
				valid := map[string]bool{"": true, "auto": true, "default": true, "over-limit": true, "flex": true, "no-limit": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Invalid service tier",
						Value: "",
					}
				}
			}
			return nil
		}),
	)
}

var _ Handler = (*NebiusHandler)(nil)
var _ CapableHandler = (*NebiusHandler)(nil)
var _ SettingsValidator = (*NebiusHandler)(nil)
var _ ModelLister = (*NebiusHandler)(nil)

func (h *NebiusHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}
