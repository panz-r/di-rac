package providers

import (
	"context"
)

// NousResearchHandler handles Nous Research API requests.
type NousResearchHandler struct {
	inner *openaiCompatHandler
}

func NewNousResearchHandler() *NousResearchHandler {
	return &NousResearchHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://inference-api.nousResearch.com/v1",
			DefaultModel: "Hermes-4-405B",
		}),
	}
}

func (h *NousResearchHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *NousResearchHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*NousResearchHandler)(nil)
