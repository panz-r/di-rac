package providers

import (
	"context"
)

// LiteLLMHandler handles LiteLLM proxy API requests.
// LiteLLM proxies to 100+ LLM providers using an OpenAI-compatible interface.
type LiteLLMHandler struct {
	inner *openaiCompatHandler
}

func NewLiteLLMHandler() *LiteLLMHandler {
	return &LiteLLMHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:     "http://localhost:4000",
			DefaultModel: "anthropic/claude-3-7-sonnet-20250219",
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				result["drop_params"] = true
				// Thinking config
				if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
					result["thinking"] = map[string]interface{}{
						"type":          "enabled",
						"budget_tokens": req.Thinking.BudgetTokens,
					}
					// Omit temperature when thinking is on for Anthropic/OAI o-models
					delete(result, "temperature")
				}
			},
		}),
	}
}

func (h *LiteLLMHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *LiteLLMHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*LiteLLMHandler)(nil)
