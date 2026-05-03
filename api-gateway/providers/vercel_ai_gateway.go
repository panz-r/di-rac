package providers

import (
	"context"
)

// VercelAIGatewayHandler handles Vercel AI Gateway API requests.
type VercelAIGatewayHandler struct {
	inner *openaiCompatHandler
}

func NewVercelAIGatewayHandler() *VercelAIGatewayHandler {
	return &VercelAIGatewayHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL: "https://ai-gateway.vercel.sh/v1",
			ExtraHeaders: map[string]string{
				"http-referer": "https://dirac.run",
				"x-title":      "Dirac",
			},
		}),
	}
}

func (h *VercelAIGatewayHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *VercelAIGatewayHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*VercelAIGatewayHandler)(nil)
