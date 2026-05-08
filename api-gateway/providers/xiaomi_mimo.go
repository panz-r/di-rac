package providers

import (
	"context"
)

// XiaomiMimoHandler handles Xiaomi MiMo API requests.
// Supports both billing modes:
//
//	Token Plan (pre-paid credits):
//	  - API keys prefixed with tp-xxxx
//	  - Region endpoints: token-plan-{sgp,ams,cn}.xiaomimimo.com/v1
//
//	Pay-As-You-Go:
//	  - API keys prefixed with sk-xxxx
//	  - Shared endpoint: api.xiaomimimo.com/v1
//
// OpenAI-compatible /chat/completions with MiMo-specific reasoning parameter.
type XiaomiMimoHandler struct {
	inner *openaiCompatHandler
}

func NewXiaomiMimoHandler() *XiaomiMimoHandler {
	const defaultModel = "mimo-v2.5-pro"
	return &XiaomiMimoHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.xiaomimimo.com/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:           "xiaomi_mimo",
				DefaultModel: defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: false,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsPromptCache:     true,
					SupportsStreaming:       true,
				},
				Settings: []ProviderSetting{
					{
						Key:   "reasoning",
						Label: "Reasoning",
						Type:  SettingSelect,
						Options: []SelectOption{
							{Value: "", Label: "Default (auto)"},
							{Value: "enabled", Label: "Enabled"},
							{Value: "disabled", Label: "Disabled"},
						},
						Group:       "reasoning",
						Description: "Enable reasoning mode (MiMo-specific).",
					},
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.01),
						Default:     0.7,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 2 = creative). Ignored in reasoning mode.",
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
						Description: "Nucleus sampling threshold. Ignored in reasoning mode.",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "context_caching",
						Label:       "Context Caching",
						Type:        SettingToggle,
						Default:     true,
						Group:       "connection",
						Description: "Enable MiMo's context caching to reduce costs for repeated prompts.",
					},
					{
						Key:    "base_url",
						Label:  "Endpoint",
						Type:   SettingSelect,
						Group:  "connection",
						Options: []SelectOption{
							{Value: "https://api.xiaomimimo.com/v1", Label: "Pay-As-You-Go (Global)"},
							{Value: "https://token-plan-sgp.xiaomimimo.com/v1", Label: "Token Plan — Singapore"},
							{Value: "https://token-plan-ams.xiaomimimo.com/v1", Label: "Token Plan — Europe"},
							{Value: "https://token-plan-cn.xiaomimimo.com/v1", Label: "Token Plan — China"},
						},
						Default:     "https://api.xiaomimimo.com/v1",
						Description: "API endpoint. Use Token Plan if your key starts with tp-.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Reasoning mode (MiMo-specific)
				reasoningVal := req.SettingString("reasoning")
				if reasoningVal == "enabled" {
					result["reasoning"] = true
				} else if reasoningVal == "disabled" {
					result["reasoning"] = false
				} else if req.Thinking != nil && req.Thinking.Type == "enabled" {
					result["reasoning"] = true
				}

				reasoningActive := reasoningVal == "enabled" ||
					(reasoningVal == "" && req.Thinking != nil && req.Thinking.Type == "enabled")

				if reasoningActive {
					delete(result, "temperature")
					delete(result, "top_p")
				} else {
					if temp := req.SettingFloat("temperature"); temp != 0 {
						result["temperature"] = temp
					}
					if topP := req.SettingFloat("top_p"); topP != 0 {
						result["top_p"] = topP
					}
				}


				if req.SettingBool("context_caching") {
					result["context_caching"] = true
				}
			},
		}),
	}
}

func (h *XiaomiMimoHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	h.applyEndpoint(req)
	return h.inner.Send(ctx, req)
}

func (h *XiaomiMimoHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	h.applyEndpoint(req)
	return h.inner.Stream(ctx, req, callback)
}

// applyEndpoint sets req.Provider.BaseURL from the endpoint setting when not overridden.
func (h *XiaomiMimoHandler) applyEndpoint(req *Request) {
	if req.Provider.BaseURL == "" {
		if endpoint := req.SettingString("base_url"); endpoint != "" {
			req.Provider.BaseURL = endpoint
		}
	}
}

func (h *XiaomiMimoHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *XiaomiMimoHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.Capabilities(), settings, thinking,
		InactiveInThinking("temperature", "top_p"),
	)
}

// ListModels returns a static list — MiMo does not expose a /models endpoint.
func (h *XiaomiMimoHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return []ModelEntry{
		{ID: "mimo-v2.5-pro", Name: "MiMo-V2.5-Pro", ContextWindow: 1048576, MaxTokens: 131072, Description: "Flagship agent/coding model (1M context, 131K output)"},
		{ID: "mimo-v2.5", Name: "MiMo-V2.5", ContextWindow: 1048576, MaxTokens: 131072, Description: "Full-modal agent model (text, image, video, audio)"},
		{ID: "mimo-v2.5-flash", Name: "MiMo-V2.5-Flash", ContextWindow: 262144, MaxTokens: 65536, Description: "Low-cost text model (256K context, high throughput)"},
		{ID: "mimo-v2-pro", Name: "MiMo-V2-Pro", ContextWindow: 131072, MaxTokens: 65536, Description: "Legacy Pro model"},
		{ID: "mimo-v2", Name: "MiMo-V2", ContextWindow: 131072, MaxTokens: 65536, Description: "Legacy model"},
	}, nil
}

var _ Handler = (*XiaomiMimoHandler)(nil)
var _ CapableHandler = (*XiaomiMimoHandler)(nil)
var _ SettingsValidator = (*XiaomiMimoHandler)(nil)
var _ ModelLister = (*XiaomiMimoHandler)(nil)
