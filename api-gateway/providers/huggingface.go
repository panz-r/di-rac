package providers

import (
	"context"
)

// HuggingFaceHandler handles Hugging Face Inference API requests.
type HuggingFaceHandler struct {
	inner *openaiCompatHandler
}

func NewHuggingFaceHandler() *HuggingFaceHandler {
	return &HuggingFaceHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://router.huggingface.co/v1",
			Capabilities: &ProviderInfo{
				ID: "huggingface",
				Features: ProviderFeatures{
					SupportsTools:     true,
					SupportsStreaming: true,
				},
				Settings: []ProviderSetting{
					{Key: "temperature", Label: "Temperature", Type: SettingSlider, Min: fPtr(0), Max: fPtr(2), Step: fPtr(0.01), Default: 1.0, Group: "sampling"},
					{Key: "top_p", Label: "Top P", Type: SettingSlider, Min: fPtr(0), Max: fPtr(1), Step: fPtr(0.01), Default: 1.0, Group: "sampling"},
					{Key: "max_tokens", Label: "Max Tokens", Type: SettingNumber, Min: fPtr(1), Group: "sampling"},
				},
			},
			DefaultModel: "moonshotai/Kimi-K2-Instruct",
		}),
	}
}

func (h *HuggingFaceHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *HuggingFaceHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*HuggingFaceHandler)(nil)

func (h *HuggingFaceHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *HuggingFaceHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

func (h *HuggingFaceHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.Capabilities(), settings, thinking)
}

var _ SettingsValidator = (*HuggingFaceHandler)(nil)

var _ CapableHandler = (*HuggingFaceHandler)(nil)
var _ ModelLister = (*HuggingFaceHandler)(nil)
