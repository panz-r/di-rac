package providers

import (
	"context"
)

// MistralHandler handles Mistral API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with Mistral-specific config:
//   - tool_choice: "any" when tools present
//   - No stream_options (Mistral sends usage without it)
//   - Content array support: delta.content can be string or [{type,text}] array
//   - Default model: devstral-2512
type MistralHandler struct {
	inner *openaiCompatHandler
}

func NewMistralHandler() *MistralHandler {
	return &MistralHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.mistral.ai/v1",
			DefaultModel:        "devstral-2512",
			ToolChoice:          "any",
			NoStreamOptions:     true,
			ContentArraySupport: true,
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Forward request temperature if explicitly set (> 0).
				// Default is 0 (set by compat handler) which matches prior behavior.
				if req.Temperature > 0 {
					result["temperature"] = req.Temperature
				}
			},
		}),
	}
}

func (h *MistralHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *MistralHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*MistralHandler)(nil)
