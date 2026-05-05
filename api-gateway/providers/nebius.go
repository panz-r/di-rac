package providers

import (
	"context"
	"strings"
)

// NebiusHandler handles Nebius AI Studio API requests.
type NebiusHandler struct {
	inner *openaiCompatHandler
}

func NewNebiusHandler() *NebiusHandler {
	return &NebiusHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.studio.nebius.ai/v1",
			DefaultModel: "Qwen/Qwen2.5-32B-Instruct-fast",
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				model := req.Provider.Model
				if req.ModelOverride != "" {
					model = req.ModelOverride
				}
				if strings.Contains(model, "DeepSeek-R1") {
					messages = openaiAddReasoningContent(messages, req)
				}
				return messages
			},
		}),
	}
}

func (h *NebiusHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *NebiusHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*NebiusHandler)(nil)

func (h *NebiusHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *NebiusHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ CapableHandler = (*NebiusHandler)(nil)
var _ ModelLister = (*NebiusHandler)(nil)
