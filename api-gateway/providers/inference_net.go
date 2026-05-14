package providers

import (
	"context"
	"net/http"
	"log"
	"net/url"
	"strings"
)

// InferenceNetHandler handles Inference.net API requests.
// Inference.net is a full-stack LLM lifecycle platform with:
//   - Base URL: https://api.inference.net/v1
//   - Model format: {org}/{model}/{quant} (e.g., google/gemma-3-27b-instruct/bf-16)
//   - Catalyst Proxy: Route to any provider via x-inference-* headers
//   - Custom Model Deployment: your-team/your-model
type InferenceNetHandler struct {
	inner *openaiCompatHandler
}

func NewInferenceNetHandler() *InferenceNetHandler {
	const defaultModel = "google/gemma-3-27b-instruct/bf-16"
	return &InferenceNetHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.inference.net/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:               "inference_net",
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
							{Value: "none", Label: "None"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Description: "Controls reasoning depth.",
					},
					// Catalyst proxy settings
					{
						Key:   "catalyst_provider",
						Label: "Provider (Catalyst)",
						Type:  SettingSelect,
						Group: "catalyst",
						Options: []SelectOption{
							{Value: "", Label: "Inference.net (direct)"},
							{Value: "openai", Label: "OpenAI"},
							{Value: "anthropic", Label: "Anthropic"},
							{Value: "groq", Label: "Groq"},
							{Value: "cerebras", Label: "Cerebras"},
							{Value: "together", Label: "Together AI"},
							{Value: "fireworks", Label: "Fireworks"},
							{Value: "replicate", Label: "Replicate"},
						},
						Description: "Route through Catalyst to this provider.",
					},
					{
						Key:         "catalyst_provider_api_key",
						Label:       "Provider API Key",
						Type:        SettingText,
						Group:       "catalyst",
						Description: "API key for the selected Catalyst provider.",
					},
					{
						Key:         "catalyst_provider_url",
						Label:       "Provider Base URL",
						Type:        SettingText,
						Group:       "catalyst",
						Description: "Custom provider base URL (for unsupported providers).",
					},
					// Observability settings
					{
						Key:         "environment",
						Label:       "Environment",
						Type:        SettingText,
						Group:       "inference_net",
						Description: "Environment label (e.g., 'production', 'staging').",
					},
					{
						Key:         "task_id",
						Label:       "Task ID",
						Type:        SettingText,
						Group:       "inference_net",
						Description: "Group requests under a logical task.",
					},
				},
			},
			ModifyHeaders: func(httpReq *http.Request, req *Request) {
				if provider := req.SettingString("catalyst_provider"); provider != "" {
					httpReq.Header.Set("x-inference-provider", provider)
				}
				if key := req.SettingString("catalyst_provider_api_key"); key != "" {
					httpReq.Header.Set("x-inference-provider-api-key", key)
				}
				if rawURL := req.SettingString("catalyst_provider_url"); rawURL != "" {
					u, err := url.Parse(rawURL)
					if err != nil || (u.Scheme != "http" && u.Scheme != "https") {
						log.Printf("[inference-net] rejected invalid catalyst_provider_url: %q", rawURL)
					} else {
						httpReq.Header.Set("x-inference-provider-url", rawURL)
					}
				}
				if env := req.SettingString("environment"); env != "" {
					httpReq.Header.Set("x-inference-environment", env)
				}
				if taskID := req.SettingString("task_id"); taskID != "" {
					httpReq.Header.Set("x-inference-task-id", taskID)
				}
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				req.ApplySettingFloat(result, "temperature")

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
					if seqs := splitStopSequences(stop); seqs != nil {
					result["stop"] = seqs
				}
				}

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				if effort := req.SettingString("reasoning_effort"); effort != "" {
					result["reasoning_effort"] = effort
				} else if req.Thinking != nil && req.Thinking.ReasoningEffort != "" {
					result["reasoning_effort"] = req.Thinking.ReasoningEffort
				}
			},
		}),
	}
}

func (h *InferenceNetHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *InferenceNetHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *InferenceNetHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *InferenceNetHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
				valid := map[string]bool{"": true, "none": true, "low": true, "medium": true, "high": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be none/low/medium/high or empty",
						Value: "",
					}
				}
			case "catalyst_provider":
				valid := map[string]bool{
					"": true, "openai": true, "anthropic": true, "groq": true,
					"cerebras": true, "together": true, "fireworks": true, "replicate": true,
				}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Invalid provider",
						Value: "",
					}
				}
			}
			return nil
		}),
	)
}

var _ Handler = (*InferenceNetHandler)(nil)
var _ CapableHandler = (*InferenceNetHandler)(nil)
var _ SettingsValidator = (*InferenceNetHandler)(nil)
var _ ModelLister = (*InferenceNetHandler)(nil)

func (h *InferenceNetHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}
