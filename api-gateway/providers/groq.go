package providers

import (
	"context"
	"strings"
)

// GroqHandler handles Groq API requests via their OpenAI-compatible endpoint.
//   - Base URL: https://api.groq.com/openai/v1
//   - Uses max_completion_tokens (not max_tokens)
//   - Reasoning models: GPT-OSS (low/medium/high), Qwen3 (none/default)
//   - reasoning_format: parsed/raw/hidden (mutually exclusive with include_reasoning)
//   - Service tiers: auto/on_demand/flex/performance
//   - DeepSeek family auto-detected for special params (top_p, reasoning_format)
//   - Cached tokens subtracted from input tokens in usage reporting
type GroqHandler struct {
	inner *openaiCompatHandler
}

func NewGroqHandler() *GroqHandler {
	const defaultModel = "qwen/qwen3-32b"
	return &GroqHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.groq.com/openai/v1",
			DefaultModel:        defaultModel,
			MaxCompletionTokens: true,
			Capabilities: &ProviderInfo{
				ID:               "groq",
				DefaultModel:     defaultModel,
				MaxTokensDefault: 16384,
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
						Default:     0.6,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 2 = creative). Recommended: 0.5–0.7 for reasoning.",
						ValidRange:  "0 – 2",
					},
					{
						Key:         "top_p",
						Label:       "Top P",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     0.95,
						Group:       "sampling",
						Description: "Nucleus sampling threshold.",
						ValidRange:  "0 – 1",
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
					{
						Key:   "response_format",
						Label: "Response Format",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "json_object", Label: "JSON Object"},
						},
						Description: "Force JSON output format.",
					},
					// Reasoning parameters
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "none", Label: "None (Qwen3)"},
							{Value: "default", Label: "Default (Qwen3)"},
							{Value: "low", Label: "Low (GPT-OSS)"},
							{Value: "medium", Label: "Medium (GPT-OSS)"},
							{Value: "high", Label: "High (GPT-OSS)"},
						},
						Description: "Controls reasoning depth. GPT-OSS: low/medium/high. Qwen3: none/default.",
					},
					{
						Key:   "reasoning_format",
						Label: "Reasoning Format",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "parsed", Label: "Parsed"},
							{Value: "raw", Label: "Raw"},
							{Value: "hidden", Label: "Hidden"},
						},
						Description: "Controls reasoning output format. Mutually exclusive with include_reasoning.",
					},
					{
						Key:         "include_reasoning",
						Label:       "Include Reasoning",
						Type:        SettingToggle,
						Scope:       ScopePerMode,
						Group:       "reasoning",
						Description: "Include reasoning in response. Mutually exclusive with reasoning_format.",
					},
					// Service tier
					{
						Key:   "service_tier",
						Label: "Service Tier",
						Type:  SettingSelect,
						Group: "provider",
						Options: []SelectOption{
							{Value: "", Label: "Default (on_demand)"},
							{Value: "auto", Label: "Auto"},
							{Value: "on_demand", Label: "On Demand"},
							{Value: "flex", Label: "Flex"},
							{Value: "performance", Label: "Performance"},
						},
						Description: "Service tier for latency/priority control.",
					},
					{
						Key:         "user",
						Label:       "User ID",
						Type:        SettingText,
						Group:       "metadata",
						Description: "End-user identifier for abuse monitoring.",
					},
					{
						Key:         "parallel_tool_calls",
						Label:       "Parallel Tool Calls",
						Type:        SettingToggle,
						Group:       "tools",
						Default:     true,
						Description: "Enable parallel function calling during tool use.",
					},
					{
						Key:   "citation_options",
						Label: "Citation Options",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default (enabled)"},
							{Value: "enabled", Label: "Enabled"},
							{Value: "disabled", Label: "Disabled"},
						},
						Description: "Whether to include citations in the response.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				req.ApplySettingFloat(result, "temperature")

				if topP := req.SettingFloat("top_p"); topP > 0 {
					result["top_p"] = topP
				}
				if seed := req.SettingInt("seed"); seed > 0 {
					result["seed"] = seed
				}
				if stop := req.SettingString("stop"); stop != "" {
					if seqs := splitStopSequences(stop); seqs != nil {
					result["stop"] = seqs
				}
				}
				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				// Reasoning
				if effort := req.SettingString("reasoning_effort"); effort != "" {
					result["reasoning_effort"] = effort
				} else if req.Thinking != nil && req.Thinking.ReasoningEffort != "" {
					result["reasoning_effort"] = req.Thinking.ReasoningEffort
				}
				if rf := req.SettingString("reasoning_format"); rf != "" {
					result["reasoning_format"] = rf
				}
				if req.SettingBool("include_reasoning") {
					result["include_reasoning"] = true
				}

				// Service tier
				if tier := req.SettingString("service_tier"); tier != "" {
					result["service_tier"] = tier
				}

				if user := req.SettingString("user"); user != "" {
					result["user"] = user
				}

				// Parallel tool calls: only send when explicitly disabled by user
				if val, ok := req.Settings["parallel_tool_calls"]; ok {
					if b, _ := val.(bool); !b {
						result["parallel_tool_calls"] = false
					}
				}

				if citations := req.SettingString("citation_options"); citations != "" {
					result["citation_options"] = citations
				}

				// DeepSeek family: auto-inject special params if not already set
				model, _ := result["model"].(string)
				family := detectGroqModelFamily(model)
				if family.specialParams != nil {
					for k, v := range family.specialParams {
						if _, exists := result[k]; !exists {
							result[k] = v
						}
					}
				}
			},
		}),
	}
}

// groqSubtractCachedTokens adjusts usage so cached tokens are subtracted
// from input tokens, as required by Groq's billing model.
func groqSubtractCachedTokens(usage *Usage) *Usage {
	if usage == nil {
		return nil
	}
	usage.InputTokens -= usage.CacheReadInputTokens
	if usage.InputTokens < 0 {
		usage.InputTokens = 0
	}
	return usage
}

func (h *GroqHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	result, err := h.inner.Send(ctx, req)
	if err != nil {
		return nil, err
	}
	result.Usage = groqSubtractCachedTokens(result.Usage)
	return result, nil
}

func (h *GroqHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, func(chunk StreamChunk) error {
		if chunk.Usage != nil {
			chunk.Usage = groqSubtractCachedTokens(chunk.Usage)
		}
		return callback(chunk)
	})
}

func (h *GroqHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

// groqModelFamily represents a Groq model family with special behavior.
type groqModelFamily struct {
	name          string
	specialParams map[string]interface{}
}

func detectGroqModelFamily(modelID string) groqModelFamily {
	switch {
	case strings.Contains(modelID, "deepseek"):
		return groqModelFamily{
			name: "DeepSeek",
			specialParams: map[string]interface{}{
				"top_p":            0.95,
				"reasoning_format": "parsed",
			},
		}
	default:
		return groqModelFamily{name: "default"}
	}
}

func (h *GroqHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.inner.config.BaseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", cfg.APIKey)
}

func (h *GroqHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
			case "response_format":
				valid := map[string]bool{"": true, "json_object": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'json_object' or empty",
						Value: "",
					}
				}
			case "reasoning_effort":
				valid := map[string]bool{"": true, "none": true, "default": true, "low": true, "medium": true, "high": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be none/default/low/medium/high or empty",
						Value: "",
					}
				}
			case "reasoning_format":
				valid := map[string]bool{"": true, "parsed": true, "raw": true, "hidden": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'parsed', 'raw', 'hidden', or empty",
						Value: "",
					}
				} else if ok && s != "" {
					// Mutual exclusivity with include_reasoning
					if ir, ok2 := allSettings["include_reasoning"].(bool); ok2 && ir {
						return &SettingValidation{
							Error: "reasoning_format and include_reasoning are mutually exclusive",
							Value: "",
						}
					}
				}
			case "include_reasoning":
				if ir, ok := val.(bool); ok && ir {
					if rf, ok := allSettings["reasoning_format"].(string); ok && rf != "" {
						return &SettingValidation{
							Error: "include_reasoning and reasoning_format are mutually exclusive",
							Value: false,
						}
					}
				}
			case "service_tier":
				valid := map[string]bool{"": true, "auto": true, "on_demand": true, "flex": true, "performance": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be auto/on_demand/flex/performance or empty",
						Value: "",
					}
				}
			case "citation_options":
				valid := map[string]bool{"": true, "enabled": true, "disabled": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'enabled', 'disabled', or empty",
						Value: "",
					}
				}
			}
			return nil
		}),
	)
}

var _ Handler = (*GroqHandler)(nil)
var _ CapableHandler = (*GroqHandler)(nil)
var _ SettingsValidator = (*GroqHandler)(nil)
var _ ModelLister = (*GroqHandler)(nil)
