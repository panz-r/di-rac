package providers

import (
	"context"
)

// LmStudioHandler handles LM Studio local API requests.
type LmStudioHandler struct {
	inner *openaiCompatHandler
}

func NewLmStudioHandler() *LmStudioHandler {
	return &LmStudioHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "http://localhost:1234/api/v0",
			MaxCompletionTokens: true,
		}),
	}
}

func (h *LmStudioHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *LmStudioHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*LmStudioHandler)(nil)

func (h *LmStudioHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *LmStudioHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ CapableHandler = (*LmStudioHandler)(nil)
var _ ModelLister = (*LmStudioHandler)(nil)
