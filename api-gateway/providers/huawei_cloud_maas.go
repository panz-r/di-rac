package providers

import (
	"context"
)

// HuaweiCloudMaaSHandler handles Huawei Cloud MaaS API requests.
type HuaweiCloudMaaSHandler struct {
	inner *openaiCompatHandler
}

func NewHuaweiCloudMaaSHandler() *HuaweiCloudMaaSHandler {
	return &HuaweiCloudMaaSHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.modelarts-maas.com/v1",
			DefaultModel:        "DeepSeek-V3",
			MaxCompletionTokens: true,
		}),
	}
}

func (h *HuaweiCloudMaaSHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *HuaweiCloudMaaSHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

var _ Handler = (*HuaweiCloudMaaSHandler)(nil)
