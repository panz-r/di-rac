package providers

import (
	"context"
	"strings"
)

// MoonshotHandler handles Moonshot AI (Kimi) API requests.
type MoonshotHandler struct {
	inner *openaiCompatHandler
	apiLine string // "china" or "international"
}

func NewMoonshotHandler() *MoonshotHandler {
	return &MoonshotHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.moonshot.cn/v1",
			DefaultModel: "kimi-k2.6",
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				model := req.Provider.Model
				if req.ModelOverride != "" {
					model = req.ModelOverride
				}
				// R1-format models need reasoning_content round-tripping
				if strings.Contains(model, "deepseek") {
					messages = openaiAddReasoningContent(messages, req)
				}
				return messages
			},
		}),
		apiLine: "china",
	}
}

func (h *MoonshotHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	h.applyDefaults(req)
	return h.inner.Send(ctx, req)
}

func (h *MoonshotHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	h.applyDefaults(req)
	return h.inner.Stream(ctx, req, callback)
}

func (h *MoonshotHandler) applyDefaults(req *Request) {
	if req.Provider.BaseURL == "" {
		switch h.apiLine {
		case "international":
			req.Provider.BaseURL = "https://api.moonshot.ai/v1"
		default:
			req.Provider.BaseURL = "https://api.moonshot.cn/v1"
		}
	}
}

var _ Handler = (*MoonshotHandler)(nil)

func (h *MoonshotHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *MoonshotHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ CapableHandler = (*MoonshotHandler)(nil)
var _ ModelLister = (*MoonshotHandler)(nil)
