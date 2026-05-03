package providers

import (
	"context"
)

// WandbHandler handles Weights & Biases Inference API requests.
type WandbHandler struct {
	inner *openaiCompatHandler
}

func NewWandbHandler() *WandbHandler {
	return &WandbHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.inference.wandb.ai/v1",
			DefaultModel: "meta-llama/Llama-3.3-70B-Instruct",
		}),
	}
}

func (h *WandbHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *WandbHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*WandbHandler)(nil)
