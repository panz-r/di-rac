package providers

import (
	"context"
	"strings"
)

// WaferHandler handles Wafer Pass API requests.
// Wafer Pass is a subscription service for optimized open-source models.
//   - Base URL: https://pass.wafer.ai/v1
//   - Model format: wafer.ai/{model} (e.g., wafer.ai/DeepSeek-V4-Pro)
//   - 4 models: DeepSeek-V4-Pro, Qwen3.5-397B-A17B, GLM-5.1, MiniMax-M2.7
//   - Thinking mode: DeepSeek-V4-Pro only (thinking: {type, budget_tokens})
//   - No /models endpoint — hardcoded model list
type WaferHandler struct {
	inner *openaiCompatHandler
}

func NewWaferHandler() *WaferHandler {
	const defaultModel = "wafer.ai/DeepSeek-V4-Pro"
	return &WaferHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://pass.wafer.ai/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:               "wafer",
				DefaultModel:     defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:  true,
					SupportsTools:     true,
					SupportsStreaming: true,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.01),
						Default:     0.7,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 2 = creative).",
						ValidRange:  "0 – 2",
					},
					{
						Key:         "top_p",
						Label:       "Top P",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     0.9,
						Group:       "sampling",
						Description: "Nucleus sampling threshold.",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "presence_penalty",
						Label:       "Presence Penalty",
						Type:        SettingSlider,
						Min:         fPtr(-2),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Group:       "sampling",
						ValidRange:  "-2 – 2",
						Description: "Penalizes new tokens based on presence in context.",
					},
					{
						Key:         "frequency_penalty",
						Label:       "Frequency Penalty",
						Type:        SettingSlider,
						Min:         fPtr(-2),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Group:       "sampling",
						ValidRange:  "-2 – 2",
						Description: "Penalizes repeated tokens.",
					},
					{
						Key:         "seed",
						Label:       "Seed",
						Type:        SettingNumber,
						Min:         fPtr(0),
						Group:       "sampling",
						Description: "Random seed for deterministic outputs.",
					},
					{
						Key:         "logprobs",
						Label:       "Log Probabilities",
						Type:        SettingToggle,
						Group:       "sampling",
						Description: "Return log probabilities of output tokens.",
					},
					{
						Key:         "top_logprobs",
						Label:       "Top Logprobs",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(20),
						Step:        fPtr(1),
						Group:       "sampling",
						ValidRange:  "0 – 20",
						Description: "Number of top log probabilities to return.",
					},
					{
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "output",
						Description: "Custom stop sequences (comma-separated, max 4).",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				req.ApplySettingFloat(result, "temperature")

				if topP := req.SettingFloat("top_p"); topP > 0 {
					result["top_p"] = topP
				}

				if pp := req.SettingFloat("presence_penalty"); pp != 0 {
					result["presence_penalty"] = pp
				}
				if fp := req.SettingFloat("frequency_penalty"); fp != 0 {
					result["frequency_penalty"] = fp
				}
				if seed := req.SettingInt("seed"); seed > 0 {
					result["seed"] = seed
				}

				logprobs := req.SettingBool("logprobs")
				if !logprobs {
					logprobs = req.Logprobs
				}
				if logprobs {
					result["logprobs"] = true
					topLogprobs := int(req.SettingFloat("top_logprobs"))
					if topLogprobs == 0 {
						topLogprobs = req.TopLogprobs
					}
					if topLogprobs > 0 {
						result["top_logprobs"] = topLogprobs
					}
				}

				if stop := req.SettingString("stop"); stop != "" {
					if seqs := splitStopSequences(stop); seqs != nil {
					result["stop"] = seqs
				}
				}

				// Wafer-specific: thinking mode (DeepSeek-V4-Pro)
				if req.Thinking != nil && req.Thinking.Type == "enabled" {
					thinking := map[string]interface{}{"type": "enabled"}
					if req.Thinking.BudgetTokens > 0 {
						thinking["budget_tokens"] = req.Thinking.BudgetTokens
					}
					result["thinking"] = thinking
					delete(result, "temperature")
					delete(result, "top_p")
				}

				// Model: ensure wafer.ai/ prefix
				if model, ok := result["model"].(string); ok && model != "" {
					if !strings.HasPrefix(model, "wafer.ai/") {
						result["model"] = "wafer.ai/" + model
					}
				}
			},
		}),
	}
}

func (h *WaferHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *WaferHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, NewThinkTagStream(callback))
}

func (h *WaferHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *WaferHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.inner.Capabilities(), settings, thinking,
		CrossParamRule(func(key string, val interface{}, allSettings map[string]interface{}) *SettingValidation {
			switch key {
			case "stop":
				if s, ok := val.(string); ok && s != "" {
					seqs := strings.Split(s, ",")
					if len(seqs) > 4 {
						return &SettingValidation{
							Error: "Maximum 4 stop sequences allowed",
						}
					}
				}
			}
			return nil
		}),
	)
}

var _ Handler = (*WaferHandler)(nil)
var _ CapableHandler = (*WaferHandler)(nil)
var _ SettingsValidator = (*WaferHandler)(nil)
var _ ModelLister = (*WaferHandler)(nil)

// ListModels returns the hardcoded Wafer model list (no /models endpoint).
func (h *WaferHandler) ListModels(_ context.Context, _ ProviderConfig) ([]ModelEntry, error) {
	return []ModelEntry{
		{
			ID:            "wafer.ai/DeepSeek-V4-Pro",
			Name:          "DeepSeek V4 Pro",
			ContextWindow: 262144,
			Description:   "Frontier coding and reasoning model. Supports thinking mode.",
		},
		{
			ID:            "wafer.ai/Qwen3.5-397B-A17B",
			Name:          "Qwen 3.5 397B",
			ContextWindow: 262144,
			Description:   "Fastest Qwen variant on Wafer's optimized stack.",
		},
		{
			ID:            "wafer.ai/GLM-5.1",
			Name:          "GLM 5.1",
			ContextWindow: 202752,
			Description:   "Z.AI flagship model.",
		},
		{
			ID:            "wafer.ai/MiniMax-M2.7",
			Name:          "MiniMax M2.7",
			ContextWindow: 204800,
			Description:   "Long-context coding and tool-use workflows.",
		},
	}, nil
}
