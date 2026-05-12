package providers

import (
	"context"
	"strings"
)

// SyntheticHandler handles Synthetic.new API requests.
// Synthetic provides OpenAI-compatible APIs with multi-provider routing.
//   - Base URL: https://api.synthetic.new/openai/v1
//   - Model prefix: hf: (required, e.g., hf:zai-org/GLM-4.7)
//   - Multi-provider routing: Fireworks, Together AI, Synthetic
//   - reasoning_effort: low/medium/high for reasoning models
//   - parallel_tool_calls, top_k, min_p supported
type SyntheticHandler struct {
	inner *openaiCompatHandler
}

func NewSyntheticHandler() *SyntheticHandler {
	const defaultModel = "hf:zai-org/GLM-4.7"
	return &SyntheticHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.synthetic.new/openai/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:               "synthetic",
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
						Default:     0.9,
						Group:       "sampling",
						Description: "Nucleus sampling threshold.",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "top_k",
						Label:       "Top K",
						Type:        SettingSlider,
						Min:         fPtr(1),
						Max:         fPtr(100),
						Step:        fPtr(1),
						Group:       "sampling",
						Description: "Limit sampling to top K tokens.",
						ValidRange:  "1 – 100",
					},
					{
						Key:         "min_p",
						Label:       "Min P",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Group:       "sampling",
						Description: "Minimum probability for nucleus sampling.",
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
						},
						Description: "Force JSON output format.",
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
						Description: "Controls reasoning depth for thinking models.",
					},
					{
						Key:         "parallel_tool_calls",
						Label:       "Parallel Tool Calls",
						Type:        SettingToggle,
						Group:       "tools",
						Description: "Enable parallel function calling during tool use.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				if req.SettingIsNull("temperature") {
					delete(result, "temperature")
				} else {
					result["temperature"] = req.SettingFloat("temperature")
				}

				if topP := req.SettingFloat("top_p"); topP > 0 {
					result["top_p"] = topP
				}
				if topK := req.SettingFloat("top_k"); topK > 0 {
					result["top_k"] = int(topK)
				}
				if minP := req.SettingFloat("min_p"); minP > 0 {
					result["min_p"] = minP
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

				if v, ok := req.SettingBoolOK("parallel_tool_calls"); ok {
					result["parallel_tool_calls"] = v
				}

				// Model: ensure hf: prefix
				if model, ok := result["model"].(string); ok && model != "" {
					if !strings.HasPrefix(model, "hf:") {
						result["model"] = "hf:" + model
					}
				}
			},
		}),
	}
}

func (h *SyntheticHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *SyntheticHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *SyntheticHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *SyntheticHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
				valid := map[string]bool{"": true, "json_object": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'json_object' or empty",
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
			}
			return nil
		}),
	)
}

var _ Handler = (*SyntheticHandler)(nil)
var _ CapableHandler = (*SyntheticHandler)(nil)
var _ SettingsValidator = (*SyntheticHandler)(nil)
var _ ModelLister = (*SyntheticHandler)(nil)

func (h *SyntheticHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}
