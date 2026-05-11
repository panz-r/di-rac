package providers

import (
	"context"
	"strings"
)

// SambaNovaHandler handles SambaNova API requests.
// Wraps the shared OpenAI-compatible handler with SambaNova-specific config.
type SambaNovaHandler struct {
	inner *openaiCompatHandler
}

func NewSambaNovaHandler() *SambaNovaHandler {
	return &SambaNovaHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:       "https://api.sambanova.ai/v1",
			Capabilities: &ProviderInfo{
				ID: "sambanova",
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
			DefaultModel:  "Meta-Llama-3.3-70B-Instruct",
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				model := strings.ToLower(req.Provider.Model)
				if req.ModelOverride != "" {
					model = strings.ToLower(req.ModelOverride)
				}
				if strings.Contains(model, "deepseek") || strings.Contains(model, "qwen3") {
					messages = openaiAddReasoningContent(messages, req)
				}
				return messages
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				if !req.SettingIsNull("temperature") {
					result["temperature"] = req.SettingFloat("temperature")
				}
				if !req.SettingIsNull("top_p") {
					result["top_p"] = req.SettingFloat("top_p")
				}
			},
		}),
	}
}

func (h *SambaNovaHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *SambaNovaHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*SambaNovaHandler)(nil)

func (h *SambaNovaHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *SambaNovaHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

func (h *SambaNovaHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.Capabilities(), settings, thinking)
}

var _ SettingsValidator = (*SambaNovaHandler)(nil)

var _ CapableHandler = (*SambaNovaHandler)(nil)
var _ ModelLister = (*SambaNovaHandler)(nil)
