package providers

import (
	"context"
	"strings"
)

// OllamaHandler handles Ollama API requests (local and cloud).
// Ollama is OpenAI-compatible, reusing openaiCompatHandler.buildRequest().
//   - Base URL: http://localhost:11434/v1 (local, default) or https://ollama.com/v1 (cloud)
//   - Authentication: Local ignores API key; Cloud requires Bearer token
//   - Model listing: /v1/models (works for both local and cloud)
//   - Ollama-specific: reasoning_effort, keep_alive
type OllamaHandler struct {
	inner *openaiCompatHandler
}

func NewOllamaHandler() *OllamaHandler {
	const defaultModel = "llama3.2"
	return &OllamaHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "http://localhost:11434/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:               "ollama",
				DefaultModel:     defaultModel,
				MaxTokensDefault: 32768,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsStreaming:       true,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.01),
						Default:     1.0,
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
						Default:     1.0,
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
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "output",
						Description: "Custom stop sequences (comma-separated, max 4).",
					},
					// Reasoning
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "none", Label: "None"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Description: "Controls reasoning depth for reasoning models.",
					},
					// Ollama-specific
					{
						Key:         "keep_alive",
						Label:       "Keep Alive",
						Type:        SettingText,
						Group:       "ollama",
						Description: "Session lifetime (e.g., '5m', '10m').",
					},
					{
						Key:         "user",
						Label:       "User ID",
						Type:        SettingText,
						Group:       "metadata",
						Description: "End-user identifier for abuse monitoring.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Temperature: override buildRequest default of 0
				result["temperature"] = req.SettingFloat("temperature")

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
				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = splitStopSequences(stop)
				}

				// Reasoning
				if effort := req.SettingString("reasoning_effort"); effort != "" {
					result["reasoning_effort"] = effort
				} else if req.Thinking != nil && req.Thinking.ReasoningEffort != "" {
					result["reasoning_effort"] = req.Thinking.ReasoningEffort
				}

				// Ollama-specific
				if keepAlive := req.SettingString("keep_alive"); keepAlive != "" {
					result["keep_alive"] = keepAlive
				}
				if user := req.SettingString("user"); user != "" {
					result["user"] = user
				}
			},
		}),
	}
}

func (h *OllamaHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *OllamaHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *OllamaHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *OllamaHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
			case "reasoning_effort":
				valid := map[string]bool{"": true, "none": true, "low": true, "medium": true, "high": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be none/low/medium/high or empty",
						Value: "",
					}
				}
			}
			return nil
		}),
	)
}

var _ Handler = (*OllamaHandler)(nil)
var _ CapableHandler = (*OllamaHandler)(nil)
var _ SettingsValidator = (*OllamaHandler)(nil)
var _ ModelLister = (*OllamaHandler)(nil)

func (h *OllamaHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}
