package providers

import (
	"context"
)

// HicapHandler handles Hicap AI API requests.
// Uses api-key header instead of Authorization: Bearer.
type HicapHandler struct {
	inner *openaiCompatHandler
}

func NewHicapHandler() *HicapHandler {
	t := float64(1)
	return &HicapHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:     "https://api.hicap.ai/v2/openai",
			Temperature: &t,
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Hicap TS does not set max_tokens
				delete(result, "max_tokens")
			},
		}),
	}
}

func (h *HicapHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	// Hicap uses api-key header, not Authorization: Bearer
	if req.Provider.APIKey != "" {
		h.inner.config.ExtraHeaders = map[string]string{
			"api-key": req.Provider.APIKey,
		}
	}
	return h.inner.Send(ctx, req)
}

func (h *HicapHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	if req.Provider.APIKey != "" {
		h.inner.config.ExtraHeaders = map[string]string{
			"api-key": req.Provider.APIKey,
		}
	}
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*HicapHandler)(nil)
