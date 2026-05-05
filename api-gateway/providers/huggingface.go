package providers

import (
	"context"
)

// HuggingFaceHandler handles Hugging Face Inference API requests.
type HuggingFaceHandler struct {
	inner *openaiCompatHandler
}

func NewHuggingFaceHandler() *HuggingFaceHandler {
	return &HuggingFaceHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://router.huggingface.co/v1",
			DefaultModel: "moonshotai/Kimi-K2-Instruct",
		}),
	}
}

func (h *HuggingFaceHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *HuggingFaceHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*HuggingFaceHandler)(nil)

func (h *HuggingFaceHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *HuggingFaceHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ CapableHandler = (*HuggingFaceHandler)(nil)
var _ ModelLister = (*HuggingFaceHandler)(nil)
