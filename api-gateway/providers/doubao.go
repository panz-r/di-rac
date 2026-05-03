package providers

import (
	"context"
)

// DoubaoHandler handles Doubao (ByteDance) API requests.
type DoubaoHandler struct {
	inner *openaiCompatHandler
}

func NewDoubaoHandler() *DoubaoHandler {
	return &DoubaoHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://ark.cn-beijing.volces.com/api/v3",
			DefaultModel:        "doubao-1-5-pro-256k-250115",
			MaxCompletionTokens: true,
		}),
	}
}

func (h *DoubaoHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *DoubaoHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*DoubaoHandler)(nil)
