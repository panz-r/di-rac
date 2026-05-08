package providers

import (
	"context"
	"strings"
)

// TogetherHandler handles Together AI API requests.
// Wraps the shared OpenAI-compatible handler with Together-specific config.
type TogetherHandler struct {
	inner *openaiCompatHandler
}

func NewTogetherHandler() *TogetherHandler {
	return &TogetherHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL: "https://api.together.xyz/v1",
			Capabilities: &ProviderInfo{
				ID: "together",
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsTools:     true,
					SupportsStreaming: true,
				},
				Settings: []ProviderSetting{
					{Key: "temperature", Label: "Temperature", Type: SettingSlider, Min: fPtr(0), Max: fPtr(2), Step: fPtr(0.01), Default: 1.0, Group: "sampling"},
					{Key: "top_p", Label: "Top P", Type: SettingSlider, Min: fPtr(0), Max: fPtr(1), Step: fPtr(0.01), Default: 1.0, Group: "sampling"},
				},
			},
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				model := req.Provider.Model
				if req.ModelOverride != "" {
					model = req.ModelOverride
				}
				if strings.Contains(model, "deepseek-reasoner") {
					messages = openaiAddReasoningContent(messages, req)
				}
				return messages
			},
		}),
	}
}

func (h *TogetherHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *TogetherHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*TogetherHandler)(nil)

func (h *TogetherHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *TogetherHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

func (h *TogetherHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.Capabilities(), settings, thinking)
}

var _ SettingsValidator = (*TogetherHandler)(nil)

var _ CapableHandler = (*TogetherHandler)(nil)
var _ ModelLister = (*TogetherHandler)(nil)
