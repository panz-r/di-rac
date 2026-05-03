package providers

import (
	"context"
)

// MiniMaxHandler wraps AnthropicHandler for MiniMax's Anthropic-compatible API.
// MiniMax exposes an Anthropic Messages API-compatible interface.
type MiniMaxHandler struct {
	inner       *AnthropicHandler
	apiLine     string // "china", "international", or "" (default: international)
}

func NewMiniMaxHandler() *MiniMaxHandler {
	return &MiniMaxHandler{
		inner:   NewAnthropicHandler(),
		apiLine: "international",
	}
}

func (h *MiniMaxHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	h.applyDefaults(req)
	return h.inner.Send(ctx, req)
}

func (h *MiniMaxHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	h.applyDefaults(req)
	return h.inner.Stream(ctx, req, callback)
}

func (h *MiniMaxHandler) applyDefaults(req *Request) {
	// MiniMax defaults to "MiniMax-M2.7" model
	if req.Provider.Model == "" {
		req.Provider.Model = "MiniMax-M2.7"
	}

	// Set base URL based on API line
	if req.Provider.BaseURL == "" {
		switch h.apiLine {
		case "china":
			req.Provider.BaseURL = "https://api.minimaxi.com/anthropic"
		default:
			req.Provider.BaseURL = "https://api.minimax.io/anthropic"
		}
	}

	// MiniMax recommends temperature 1.0 with range (0.0, 1.0]
	// When thinking is enabled, temperature must be omitted
	thinkingOn := req.Thinking != nil && req.Thinking.BudgetTokens > 0
	if !thinkingOn && req.Temperature == 0 {
		req.Temperature = 1.0
	}
}
