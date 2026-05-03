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
			BaseURL:  "https://api.together.xyz/v1",
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
