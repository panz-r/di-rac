package providers

import (
	"context"
	"strings"
)

// OpenAIHandler handles OpenAI API requests via the shared openaiCompatHandler.
type OpenAIHandler struct {
	inner *openaiCompatHandler
}

func NewOpenAIHandler() *OpenAIHandler {
	const defaultModel = "gpt-4o"
	return &OpenAIHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.openai.com/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:           "openai",
				DefaultModel: defaultModel,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsPromptCache:     false,
					SupportsStreaming:       true,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Default:     1.0,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 2 = creative). Ignored in thinking mode.",
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
						Description: "Nucleus sampling threshold. Ignored in thinking mode.",
						ValidRange:  "0 – 1",
					},
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Group:       "reasoning",
						Description: "Controls reasoning depth for o-series models. Only applies in thinking mode.",
					},
					{
						Key:         "max_completion_tokens",
						Label:       "Max Completion Tokens",
						Type:        SettingNumber,
						Min:         fPtr(1),
						Group:       "sampling",
						Description: "Maximum tokens in the completion.",
					},
					{
						Key:        "presence_penalty",
						Label:      "Presence Penalty",
						Type:       SettingSlider,
						Min:        fPtr(-2),
						Max:        fPtr(2),
						Step:       fPtr(0.1),
						Group:      "sampling",
						ValidRange: "-2 – 2",
					},
					{
						Key:        "frequency_penalty",
						Label:      "Frequency Penalty",
						Type:       SettingSlider,
						Min:        fPtr(-2),
						Max:        fPtr(2),
						Step:       fPtr(0.1),
						Group:      "sampling",
						ValidRange: "-2 – 2",
					},
					{
						Key:         "logprobs",
						Label:       "Log Probabilities",
						Type:        SettingToggle,
						Group:       "sampling",
						Description: "Return log probabilities of output tokens.",
					},
					{
						Key:        "top_logprobs",
						Label:      "Top Logprobs",
						Type:       SettingSlider,
						Min:        fPtr(0),
						Max:        fPtr(20),
						Step:       fPtr(1),
						Group:      "sampling",
						ValidRange: "0 – 20",
					},
					{
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "sampling",
						Description: "Custom stop sequences (comma-separated, max 4).",
					},
				},
			},
		}),
	}
}

func (h *OpenAIHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *OpenAIHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *OpenAIHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *OpenAIHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.inner.config.BaseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", cfg.APIKey)
}

var _ CapableHandler = (*OpenAIHandler)(nil)
var _ ModelLister = (*OpenAIHandler)(nil)
