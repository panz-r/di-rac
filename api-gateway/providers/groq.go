package providers

import (
	"context"
	"strings"
)

// GroqHandler handles Groq API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with Groq-specific config:
//   - Model family detection for special params (e.g. DeepSeek: reasoning_format, top_p)
//   - Cached tokens subtracted from input tokens in usage reporting
//   - Temperature hardcoded to 0
//   - max_tokens (not max_completion_tokens)
type GroqHandler struct {
	inner *openaiCompatHandler
}

func NewGroqHandler() *GroqHandler {
	const defaultModel = "moonshotai/kimi-k2-instruct-0905"
	return &GroqHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.groq.com/openai/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:           "groq",
				DefaultModel: defaultModel,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: false,
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
						Step:        fPtr(0.01),
						Default:     0,
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
						Key:         "max_tokens",
						Label:       "Max Tokens",
						Type:        SettingNumber,
						Min:         fPtr(1),
						Group:       "sampling",
						Description: "Maximum number of tokens to generate.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				model, _ := result["model"].(string)
				family := detectGroqModelFamily(model)
				if family.specialParams != nil {
					for k, v := range family.specialParams {
						// Don't override user-specified values
						if _, exists := result[k]; !exists {
							result[k] = v
						}
					}
				}
			},
		}),
	}
}

// groqSubtractCachedTokens adjusts usage so that cached tokens are subtracted
// from input tokens, as required by Groq's billing model.
func groqSubtractCachedTokens(usage *Usage) *Usage {
	if usage == nil {
		return nil
	}
	usage.InputTokens -= usage.CacheReadInputTokens
	return usage
}

func (h *GroqHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	result, err := h.inner.Send(ctx, req)
	if err != nil {
		return nil, err
	}
	result.Usage = groqSubtractCachedTokens(result.Usage)
	return result, nil
}

// Stream wraps the inner handler to subtract cached tokens from input tokens in usage chunks.
func (h *GroqHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, func(chunk StreamChunk) error {
		if chunk.Usage != nil {
			chunk.Usage = groqSubtractCachedTokens(chunk.Usage)
		}
		return callback(chunk)
	})
}

func (h *GroqHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// groqModelFamily represents a Groq model family with special behavior.
type groqModelFamily struct {
	name          string
	specialParams map[string]interface{}
}

func detectGroqModelFamily(modelID string) groqModelFamily {
	switch {
	case strings.Contains(modelID, "deepseek"):
		return groqModelFamily{
			name: "DeepSeek",
			specialParams: map[string]interface{}{
				"top_p":            0.95,
				"reasoning_format": "parsed",
			},
		}
	default:
		return groqModelFamily{name: "default"}
	}
}

func (h *GroqHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.inner.config.BaseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", cfg.APIKey)
}

var _ Handler = (*GroqHandler)(nil)
var _ CapableHandler = (*GroqHandler)(nil)
var _ ModelLister = (*GroqHandler)(nil)
