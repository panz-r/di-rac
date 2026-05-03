package providers

import (
	"context"
)

// DeepSeekHandler handles DeepSeek API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with DeepSeek-specific config:
//   - max_completion_tokens instead of max_tokens
//   - strict: true on all tool function definitions
//   - R1 format: reasoning_content on assistant messages, coerced non-null
//   - Thinking mode: omit temperature/top_p, enable thinking parameter
//   - Default model: deepseek-chat
type DeepSeekHandler struct {
	inner *openaiCompatHandler
}

func fPtr(v float64) *float64 { return &v }

func NewDeepSeekHandler() *DeepSeekHandler {
	const defaultModel = "deepseek-chat"
	return &DeepSeekHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.deepseek.com/v1",
			DefaultModel:        defaultModel,
			MaxCompletionTokens: true,
			StrictTools:         true,
			Capabilities: &ProviderInfo{
				ID:           "deepseek",
				DefaultModel: defaultModel,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          false,
					SupportsPromptCache:     true,
					SupportsStreaming:       true,
				},
				Settings: []ProviderSetting{
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "high", Label: "High"},
							{Value: "max", Label: "Max"},
						},
						Group:       "reasoning",
						Description: "Controls reasoning depth for thinking mode",
					},
					{
						Key:         "logprobs",
						Label:       "Log Probabilities",
						Type:        SettingToggle,
						Group:       "sampling",
						Description: "Return log probabilities of output tokens",
					},
					{
						Key:   "top_logprobs",
						Label: "Top Logprobs",
						Type:  SettingSlider,
						Min:   fPtr(0),
						Max:   fPtr(20),
						Step:  fPtr(1),
						Group: "sampling",
					},
					{
						Key:   "presence_penalty",
						Label: "Presence Penalty",
						Type:  SettingSlider,
						Min:   fPtr(-2),
						Max:   fPtr(2),
						Step:  fPtr(0.1),
						Group: "sampling",
					},
					{
						Key:   "frequency_penalty",
						Label: "Frequency Penalty",
						Type:  SettingSlider,
						Min:   fPtr(-2),
						Max:   fPtr(2),
						Step:  fPtr(0.1),
						Group: "sampling",
					},
				},
			},
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				messages = openaiAddReasoningContent(messages, req)
				messages = coerceAssistantMessages(messages)
				return messages
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				if req.Thinking != nil && req.Thinking.Type == "enabled" {
					// DeepSeek thinking mode
					result["thinking"] = map[string]interface{}{
						"type": req.Thinking.Type,
					}
					if req.Thinking.ReasoningEffort != "" {
						result["reasoning_effort"] = req.Thinking.ReasoningEffort
					}
					// Thinking mode ignores temperature/top_p
					delete(result, "temperature")
					delete(result, "top_p")
				} else {
					result["temperature"] = 0
				}
				// DeepSeek-specific general params
				if req.Logprobs {
					result["logprobs"] = true
					if req.TopLogprobs > 0 {
						result["top_logprobs"] = req.TopLogprobs
					}
				}
				if req.PresencePenalty != 0 {
					result["presence_penalty"] = req.PresencePenalty
				}
				if req.FrequencyPenalty != 0 {
					result["frequency_penalty"] = req.FrequencyPenalty
				}
			},
		}),
	}
}

func (h *DeepSeekHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *DeepSeekHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *DeepSeekHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

var _ CapableHandler = (*DeepSeekHandler)(nil)

// coerceAssistantMessages ensures all assistant messages have non-null content
// and non-null reasoning_content, as required by DeepSeek.
func coerceAssistantMessages(messages []map[string]interface{}) []map[string]interface{} {
	for _, msg := range messages {
		role, _ := msg["role"].(string)
		if role != "assistant" {
			continue
		}
		if msg["content"] == nil {
			msg["content"] = ""
		}
		if _, ok := msg["reasoning_content"]; !ok {
			msg["reasoning_content"] = ""
		}
	}
	return messages
}
