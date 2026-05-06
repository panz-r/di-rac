package providers

import (
	"context"
	"strings"
)

// FireworksHandler handles Fireworks AI API requests.
// Fireworks uses an OpenAI-compatible chat completions API with:
//   - Base URL: https://api.fireworks.ai/inference/v1
//   - R1 format for models marked isR1FormatRequired (addReasoningContent)
//   - <think/> tag detection in content for reasoning extraction
//   - reasoning_content field in streaming deltas
//   - prompt_cache_hit_tokens / prompt_cache_miss_tokens in usage
type FireworksHandler struct {
	inner *openaiCompatHandler
}

func NewFireworksHandler() *FireworksHandler {
	const defaultModel = "accounts/fireworks/models/kimi-k2p6"
	return &FireworksHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.fireworks.ai/inference/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:           "fireworks",
				DefaultModel: defaultModel,
				Features: ProviderFeatures{
					SupportsThinking:    true,
					SupportsTools:       true,
					SupportsImages:      true,
					SupportsPromptCache: false,
					SupportsStreaming:   true,
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
						Key:         "max_tokens",
						Label:       "Max Tokens",
						Type:        SettingNumber,
						Min:         fPtr(1),
						Group:       "sampling",
						Description: "Maximum tokens in the response.",
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
				},
			},
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				return openaiAddReasoningContent(messages, req)
			},
		}),
	}
}

func (h *FireworksHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

// Stream delegates to inner but wraps the callback to handle <think/> tag
// detection in content. Some Fireworks models emit <think/> tags in content
// rather than in the reasoning_content field, which need to be extracted and
// emitted as Thinking deltas.
func (h *FireworksHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	var reasoningAccum *strings.Builder
	return h.inner.Stream(ctx, req, func(chunk StreamChunk) error {
		if chunk.Type == "delta" && chunk.TextDelta != "" && chunk.Thinking == "" {
			content := chunk.TextDelta
			if reasoningAccum != nil || strings.Contains(content, "<think") {
				if reasoningAccum == nil {
					reasoningAccum = &strings.Builder{}
				}
				reasoningAccum.WriteString(content)

				// Emit as Thinking delta
				return callback(StreamChunk{Type: "delta", Thinking: content})
			}
		} else if chunk.Type == "delta" && chunk.Thinking != "" {
			// Track <think/> state through reasoning_content too
			if reasoningAccum != nil || strings.Contains(chunk.Thinking, "<think") {
				if reasoningAccum == nil {
					reasoningAccum = &strings.Builder{}
				}
				reasoningAccum.WriteString(chunk.Thinking)
			}
		}

		// Check for </think/> to end reasoning tracking
		if reasoningAccum != nil && strings.Contains(reasoningAccum.String(), "</think") {
			reasoningAccum = nil
		}

		return callback(chunk)
	})
}

func (h *FireworksHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

var _ CapableHandler = (*FireworksHandler)(nil)

func (h *FireworksHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ ModelLister = (*FireworksHandler)(nil)
