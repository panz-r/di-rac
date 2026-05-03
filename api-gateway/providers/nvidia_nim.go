package providers

import (
	"context"
)

// NvidiaNimHandler handles NVIDIA NIM API requests.
type NvidiaNimHandler struct {
	inner *openaiCompatHandler
}

func NewNvidiaNimHandler() *NvidiaNimHandler {
	return &NvidiaNimHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://integrate.api.nvidia.com/v1",
			DefaultModel: "nvidia/llama-3.1-nemotron-ultra-253b-v1",
		}),
	}
}

func (h *NvidiaNimHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *NvidiaNimHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*NvidiaNimHandler)(nil)
