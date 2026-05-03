package providers

import (
	"context"
	"net/http"
)

// OpenRouterHandler wraps OpenAIHandler with OpenRouter-specific base URL and headers.
type OpenRouterHandler struct {
	inner *OpenAIHandler
}

func NewOpenRouterHandler() *OpenRouterHandler {
	return &OpenRouterHandler{
		inner: &OpenAIHandler{
			httpClient: &http.Client{},
			baseURL:    "https://openrouter.ai/api/v1",
		},
	}
}

func NewOpenRouterHandlerWithKey(apiKey string) *OpenRouterHandler {
	return &OpenRouterHandler{
		inner: &OpenAIHandler{
			httpClient: &http.Client{},
			baseURL:    "https://openrouter.ai/api/v1",
			apiKey:     apiKey,
		},
	}
}

func (h *OpenRouterHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *OpenRouterHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}
