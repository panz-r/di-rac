package providers

import (
	"context"
	"strings"
)

// OVHcloudHandler handles OVHcloud AI Endpoints API requests.
// OVHcloud uses model-specific subdomain endpoints (no single base URL).
//   - URL format: {model-id}.endpoints.kepler.ai.cloud.ovh.net/api/openai_compat/v1
//   - No /v1/models endpoint — hardcoded catalog returned instead
//   - GDPR-compliant, EU data centers (Gravelines, France)
//   - Rate limit: 400 RPM per project per model
type OVHcloudHandler struct {
	inner *openaiCompatHandler
}

func NewOVHcloudHandler() *OVHcloudHandler {
	const defaultModel = "gpt-oss-120b"
	return &OVHcloudHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "", // intentionally empty; set dynamically per-model in Send/Stream
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:               "ovhcloud",
				DefaultModel:     defaultModel,
				MaxTokensDefault: 131072,
				Features: ProviderFeatures{
					SupportsTools:     true,
					SupportsImages:    true,
					SupportsStreaming: true,
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
						Key:         "user",
						Label:       "User ID",
						Type:        SettingText,
						Group:       "metadata",
						Description: "End-user identifier for abuse monitoring.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
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
				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = splitStopSequences(stop)
				}
				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}
				if user := req.SettingString("user"); user != "" {
					result["user"] = user
				}
			},
		}),
	}
}

// ovhcloudBaseURL constructs the model-specific base URL.
func ovhcloudBaseURL(model string) string {
	return "https://" + model + ".endpoints.kepler.ai.cloud.ovh.net/api/openai_compat/v1"
}

func (h *OVHcloudHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = h.inner.config.DefaultModel
	}
	req.Provider.BaseURL = ovhcloudBaseURL(model)
	return h.inner.Send(ctx, req)
}

func (h *OVHcloudHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = h.inner.config.DefaultModel
	}
	req.Provider.BaseURL = ovhcloudBaseURL(model)
	return h.inner.Stream(ctx, req, callback)
}

func (h *OVHcloudHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *OVHcloudHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
			}
			return nil
		}),
	)
}

var _ Handler = (*OVHcloudHandler)(nil)
var _ CapableHandler = (*OVHcloudHandler)(nil)
var _ SettingsValidator = (*OVHcloudHandler)(nil)

func (h *OVHcloudHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return []ModelEntry{
		{ID: "gpt-oss-120b", Name: "GPT-OSS 120B", ContextWindow: 131072},
		{ID: "gpt-oss-20b", Name: "GPT-OSS 20B", ContextWindow: 131072},
		{ID: "qwen3-32b", Name: "Qwen3 32B", ContextWindow: 32768},
		{ID: "meta-llama-3_3-70b-instruct", Name: "Meta Llama 3.3 70B Instruct", ContextWindow: 131072},
		{ID: "mistral-7b-instruct-v0.3", Name: "Mistral 7B Instruct v0.3", ContextWindow: 127000},
		{ID: "mistral-small-3.2-24b-instruct", Name: "Mistral Small 3.2 24B Instruct", ContextWindow: 128000},
		{ID: "qwen2.5-vl-72b-instruct", Name: "Qwen2.5 VL 72B Instruct", ContextWindow: 32768},
		{ID: "qwen3-coder-30b-a3b-instruct", Name: "Qwen3 Coder 30B A3B Instruct", ContextWindow: 262144},
	}, nil
}

var _ ModelLister = (*OVHcloudHandler)(nil)
