package providers

import (
	"context"
)

// LmStudioHandler handles LM Studio local API requests.
type LmStudioHandler struct {
	inner *openaiCompatHandler
}

func NewLmStudioHandler() *LmStudioHandler {
	return &LmStudioHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "http://localhost:1234/api/v0",
			MaxCompletionTokens: true,
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				if !req.SettingIsNull("temperature") {
					result["temperature"] = req.SettingFloat("temperature")
				}
				if !req.SettingIsNull("top_p") {
					result["top_p"] = req.SettingFloat("top_p")
				}
			},
			Capabilities: &ProviderInfo{
				ID:               "lmstudio",
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsTools:     true,
					SupportsStreaming: true,
					SupportsImages:    true,
				},
				Settings: []ProviderSetting{
					{Key: "temperature", Label: "Temperature", Type: SettingSlider, Min: fPtr(0), Max: fPtr(2), Step: fPtr(0.01), Default: 1.0, Group: "sampling"},
					{Key: "top_p", Label: "Top P", Type: SettingSlider, Min: fPtr(0), Max: fPtr(1), Step: fPtr(0.01), Default: 1.0, Group: "sampling"},
				},
			},
		}),
	}
}

func (h *LmStudioHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *LmStudioHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*LmStudioHandler)(nil)

func (h *LmStudioHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *LmStudioHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

func (h *LmStudioHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.Capabilities(), settings, thinking)
}

var _ SettingsValidator = (*LmStudioHandler)(nil)

var _ CapableHandler = (*LmStudioHandler)(nil)
var _ ModelLister = (*LmStudioHandler)(nil)
