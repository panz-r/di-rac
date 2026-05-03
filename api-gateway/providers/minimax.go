package providers

import (
	"context"
)

// MiniMaxHandler handles MiniMax API requests via their native OpenAI-compatible endpoint.
type MiniMaxHandler struct {
	inner *openaiCompatHandler
}

func NewMiniMaxHandler() *MiniMaxHandler {
	temp := 1.0
	return &MiniMaxHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.minimax.io/v1",
			DefaultModel: "MiniMax-M2.7",
			Temperature:  &temp,
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Split thinking into reasoning_details field for cleaner parsing
				result["reasoning_split"] = true
			},
		}),
	}
}

func (h *MiniMaxHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *MiniMaxHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*MiniMaxHandler)(nil)
