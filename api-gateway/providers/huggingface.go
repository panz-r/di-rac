package providers

import (
	"context"
	"strings"
)

// HuggingFaceHandler handles Hugging Face Inference API requests.
// Hugging Face Inference Providers is a unified API for 20+ providers.
//   - Base URL: https://router.huggingface.co/v1
//   - Model format: {provider}/{model} (e.g., moonshotai/Kimi-K2-Instruct)
//   - Provider selection suffixes: :fastest, :cheapest, :preferred, :{provider}
//   - reasoning_effort: none/minimal/low/medium/high/xhigh
//   - top_logprobs max is 5 (not 20 like OpenAI)
//   - Free tier available with generous limits
type HuggingFaceHandler struct {
	inner *openaiCompatHandler
}

func NewHuggingFaceHandler() *HuggingFaceHandler {
	const defaultModel = "moonshotai/Kimi-K2-Instruct"
	return &HuggingFaceHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://router.huggingface.co/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:               "huggingface",
				DefaultModel:     defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:        true,
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
						Max:         fPtr(5),
						Step:        fPtr(1),
						Group:       "sampling",
						ValidRange:  "0 – 5",
						Description: "Number of top log probabilities to return (max 5).",
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
							{Value: "json_schema", Label: "JSON Schema"},
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
							{Value: "none", Label: "None"},
							{Value: "minimal", Label: "Minimal"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
							{Value: "xhigh", Label: "XHigh"},
						},
						Description: "Controls reasoning depth (model-dependent).",
					},
					{
						Key:   "provider",
						Label: "Provider",
						Type:  SettingSelect,
						Group: "huggingface",
						Options: []SelectOption{
							{Value: "", Label: "Auto (fastest)"},
							{Value: "fastest", Label: "Fastest"},
							{Value: "cheapest", Label: "Cheapest"},
							{Value: "preferred", Label: "Preferred"},
							{Value: "together", Label: "Together AI"},
							{Value: "groq", Label: "Groq"},
							{Value: "fireworks", Label: "Fireworks"},
							{Value: "cerebras", Label: "Cerebras"},
							{Value: "sambanova", Label: "SambaNova"},
							{Value: "replicate", Label: "Replicate"},
							{Value: "fal", Label: "Fal AI"},
							{Value: "hf-inference", Label: "HF Inference"},
						},
						Description: "Provider selection policy or explicit provider.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				if !req.SettingIsNull("temperature") {
					result["temperature"] = req.SettingFloat("temperature")
				}

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

				// Reasoning effort
				if effort := req.SettingString("reasoning_effort"); effort != "" {
					result["reasoning_effort"] = effort
				} else if req.Thinking != nil && req.Thinking.ReasoningEffort != "" {
					result["reasoning_effort"] = req.Thinking.ReasoningEffort
				}

				// Provider selection: append :{provider} suffix to model ID
				if provider := req.SettingString("provider"); provider != "" {
					if model, ok := result["model"].(string); ok && model != "" {
						result["model"] = model + ":" + provider
					}
				}
			},
		}),
	}
}

func (h *HuggingFaceHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *HuggingFaceHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *HuggingFaceHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *HuggingFaceHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
				valid := map[string]bool{"": true, "json_object": true, "json_schema": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'json_object', 'json_schema', or empty",
						Value: "",
					}
				}
			case "reasoning_effort":
				valid := map[string]bool{"": true, "none": true, "minimal": true, "low": true, "medium": true, "high": true, "xhigh": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be none/minimal/low/medium/high/xhigh",
						Value: "",
					}
				}
			case "provider":
				valid := map[string]bool{
					"": true, "fastest": true, "cheapest": true, "preferred": true,
					"together": true, "groq": true, "fireworks": true, "cerebras": true,
					"sambanova": true, "replicate": true, "fal": true, "hf-inference": true,
				}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Invalid provider selection",
						Value: "",
					}
				}
			case "top_logprobs":
				if num := toFloat(val); num > 5 {
					return &SettingValidation{
						Error: "Max 5 for Hugging Face (not 20 like OpenAI)",
						Value: float64(5),
					}
				}
			}
			return nil
		}),
	)
}

var _ Handler = (*HuggingFaceHandler)(nil)
var _ CapableHandler = (*HuggingFaceHandler)(nil)
var _ SettingsValidator = (*HuggingFaceHandler)(nil)
var _ ModelLister = (*HuggingFaceHandler)(nil)

func (h *HuggingFaceHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}
