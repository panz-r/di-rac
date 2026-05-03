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
