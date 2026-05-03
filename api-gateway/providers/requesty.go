package providers

import (
	"context"
	"strings"
)

// RequestyHandler handles Requesty AI router API requests.
type RequestyHandler struct {
	inner *openaiCompatHandler
}

func NewRequestyHandler() *RequestyHandler {
	return &RequestyHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://router.requesty.ai/v1",
			DefaultModel: "anthropic/claude-3-7-sonnet-latest",
			ExtraHeaders: map[string]string{
				"HTTP-Referer": "https://dirac.run",
				"X-Title":      "Dirac",
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				model, _ := result["model"].(string)
				// reasoning_effort for OpenAI o-models
				if strings.HasPrefix(model, "openai/o") {
					result["reasoning_effort"] = "medium"
				}
				// Thinking config for Claude models
				if isClaudeModel(model) {
					if req.Thinking != nil && req.Thinking.BudgetTokens > 0 {
						result["thinking"] = map[string]interface{}{
							"type":          "enabled",
							"budget_tokens": req.Thinking.BudgetTokens,
						}
						delete(result, "temperature")
					} else {
						result["thinking"] = map[string]interface{}{
							"type": "disabled",
						}
					}
				}
			},
		}),
	}
}

func (h *RequestyHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *RequestyHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func isClaudeModel(model string) bool {
	return strings.Contains(model, "claude-opus-4") ||
		strings.Contains(model, "claude-sonnet-4") ||
		strings.Contains(model, "claude-4.6-sonnet") ||
		strings.Contains(model, "claude-3-7-sonnet")
}

var _ Handler = (*RequestyHandler)(nil)
