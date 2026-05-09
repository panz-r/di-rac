package providers

import (
	"context"
	"strings"
)

// FireworksHandler handles Fireworks AI API requests.
// Fireworks uses an OpenAI-compatible chat completions API with:
//   - Base URL: https://api.fireworks.ai/inference/v1
//   - R1 format for models marked isR1FormatRequired (addReasoningContent)
//   - <think/> tag detection in content for reasoning extraction
//   - reasoning_content field in streaming deltas
//   - prompt_cache_hit_tokens / prompt_cache_miss_tokens in usage
//   - reasoning_effort: none/low/medium/high/xhigh/max
//   - thinking with budget_tokens and keep options
type FireworksHandler struct {
	inner *openaiCompatHandler
}

func NewFireworksHandler() *FireworksHandler {
	const defaultModel = "accounts/fireworks/models/kimi-k2p6"
	return &FireworksHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.fireworks.ai/inference/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:               "fireworks",
				DefaultModel:     defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsPromptCache:     true,
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
						Description: "Controls randomness (0 = deterministic, 2 = creative). Ignored in thinking mode.",
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
						Description: "Nucleus sampling threshold. Ignored in thinking mode.",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "top_k",
						Label:       "Top K",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(256),
						Step:        fPtr(1),
						Group:       "sampling",
						Description: "Limits sampling to top K tokens. 0 = disabled. Ignored in thinking mode.",
						ValidRange:  "0 – 256",
					},
					{
						Key:         "min_p",
						Label:       "Min P",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Group:       "sampling",
						Description: "Minimum probability threshold. 0 = disabled. Ignored in thinking mode.",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "typical_p",
						Label:       "Typical P",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Group:       "sampling",
						Description: "Locally typical sampling threshold. 0 = disabled. Ignored in thinking mode.",
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
						Description: "Penalizes new tokens based on presence in context. Ignored in thinking mode.",
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
						Description: "Penalizes repeated tokens. Ignored in thinking mode.",
					},
					{
						Key:         "repetition_penalty",
						Label:       "Repetition Penalty",
						Type:        SettingSlider,
						Min:         fPtr(1),
						Max:         fPtr(2),
						Step:        fPtr(0.01),
						Group:       "sampling",
						ValidRange:  "1 – 2",
						Description: "Penalizes repetition (1 = disabled). Ignored in thinking mode.",
					},
					{
						Key:         "seed",
						Label:       "Seed",
						Type:        SettingNumber,
						Min:         fPtr(0),
						Group:       "sampling",
						Description: "Deterministic sampling seed.",
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
					{
						Key:   "response_format",
						Label: "Response Format",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "json_object", Label: "JSON Object"},
							{Value: "json_schema", Label: "JSON Schema"},
						},
						Description: "Force structured output format.",
					},
					{
						Key:   "thinking_type",
						Label: "Thinking Mode",
						Type:  SettingSelect,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "enabled", Label: "Enabled"},
							{Value: "disabled", Label: "Disabled"},
						},
						Description: "Enable thinking mode for reasoning models.",
					},
					{
						Key:         "thinking_budget_tokens",
						Label:       "Thinking Budget",
						Type:        SettingNumber,
						Min:         fPtr(1),
						Group:       "reasoning",
						Description: "Token budget for thinking. Only applies in thinking mode.",
					},
					{
						Key:   "thinking_keep",
						Label: "Thinking Keep",
						Type:  SettingSelect,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default (last)"},
							{Value: "last", Label: "Last"},
							{Value: "all", Label: "All"},
							{Value: "none", Label: "None"},
						},
						Description: "Which thinking turns to keep in multi-turn. Only applies in thinking mode.",
					},
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
							{Value: "xhigh", Label: "X-High"},
							{Value: "max", Label: "Max"},
						},
						Description: "Controls reasoning depth. Only applies in thinking mode.",
					},
					{
						Key:   "reasoning_history",
						Label: "Reasoning History",
						Type:  SettingSelect,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default (interleaved)"},
							{Value: "disabled", Label: "Disabled"},
							{Value: "interleaved", Label: "Interleaved"},
							{Value: "preserved", Label: "Preserved"},
						},
						Description: "How reasoning content is included in multi-turn. Only applies in thinking mode.",
					},
					{
						Key:         "prompt_cache_key",
						Label:       "Prompt Cache Key",
						Type:        SettingText,
						Group:       "provider",
						Description: "Cache key for prompt caching (prefix-based matching).",
					},
					{
						Key:         "prompt_cache_isolation_key",
						Label:       "Cache Isolation Key",
						Type:        SettingText,
						Group:       "provider",
						Description: "Isolation key for prompt cache partitioning.",
					},
					{
						Key:   "service_tier",
						Label: "Service Tier",
						Type:  SettingSelect,
						Group: "provider",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "priority", Label: "Priority"},
						},
						Description: "Service tier for request priority.",
					},
					{
						Key:   "context_length_exceeded_behavior",
						Label: "Context Overflow",
						Type:  SettingSelect,
						Group: "provider",
						Options: []SelectOption{
							{Value: "", Label: "Default (error)"},
							{Value: "truncate", Label: "Truncate"},
							{Value: "error", Label: "Error"},
						},
						Description: "Behavior when context exceeds model limits.",
					},
					{
						Key:         "user",
						Label:       "User ID",
						Type:        SettingText,
						Group:       "provider",
						Description: "End-user identifier for abuse monitoring.",
					},
				},
			},
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				return openaiAddReasoningContent(messages, req)
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				thinkingType := req.SettingString("thinking_type")
				if thinkingType == "" && req.Thinking != nil && req.Thinking.Type == "enabled" {
					thinkingType = "enabled"
				}

				thinkingActive := thinkingType == "enabled"

				if thinkingActive {
					delete(result, "temperature")
					delete(result, "top_p")
					thinking := map[string]interface{}{"type": "enabled"}
					if budget := int(req.SettingFloat("thinking_budget_tokens")); budget > 0 {
						thinking["budget_tokens"] = budget
					}
					if keep := req.SettingString("thinking_keep"); keep != "" {
						thinking["keep"] = keep
					}
					result["thinking"] = thinking

					effort := req.SettingString("reasoning_effort")
					if effort == "" && req.Thinking != nil && req.Thinking.ReasoningEffort != "" {
						effort = req.Thinking.ReasoningEffort
					}
					if effort != "" {
						result["reasoning_effort"] = effort
					}
				} else {
					result["temperature"] = req.SettingFloat("temperature")

					if topP := req.SettingFloat("top_p"); topP > 0 {
						result["top_p"] = topP
					}

					if topK := req.SettingFloat("top_k"); topK > 0 {
						result["top_k"] = int(topK)
					}

					if minP := req.SettingFloat("min_p"); minP > 0 {
						result["min_p"] = minP
					}
					if typicalP := req.SettingFloat("typical_p"); typicalP > 0 {
						result["typical_p"] = typicalP
					}

					if pp := req.SettingFloat("presence_penalty"); pp != 0 {
						result["presence_penalty"] = pp
					}
					if fp := req.SettingFloat("frequency_penalty"); fp != 0 {
						result["frequency_penalty"] = fp
					}
					if rp := req.SettingFloat("repetition_penalty"); rp > 1 {
						result["repetition_penalty"] = rp
					}

					if seed := req.SettingFloat("seed"); seed > 0 {
						result["seed"] = int(seed)
					}

					delete(result, "reasoning_effort")
				}

				if logprobs := req.SettingBool("logprobs"); logprobs {
					result["logprobs"] = true
					if topLogprobs := int(req.SettingFloat("top_logprobs")); topLogprobs > 0 {
						result["top_logprobs"] = topLogprobs
					}
				}

				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = splitStopSequences(stop)
				}

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				if rh := req.SettingString("reasoning_history"); rh != "" {
					result["reasoning_history"] = rh
				}

				if cacheKey := req.SettingString("prompt_cache_key"); cacheKey != "" {
					result["prompt_cache_key"] = cacheKey
				}

				if isolationKey := req.SettingString("prompt_cache_isolation_key"); isolationKey != "" {
					result["prompt_cache_isolation_key"] = isolationKey
				}

				if tier := req.SettingString("service_tier"); tier != "" {
					result["service_tier"] = tier
				}

				if behavior := req.SettingString("context_length_exceeded_behavior"); behavior != "" {
					result["context_length_exceeded_behavior"] = behavior
				}

				if user := req.SettingString("user"); user != "" {
					result["user"] = user
				}
			},
		}),
	}
}

func (h *FireworksHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

// Stream delegates to inner but wraps the callback to handle <think/> tag
// detection in content. Some Fireworks models emit <think/> tags in content
// rather than in the reasoning_content field, which need to be extracted and
// emitted as Thinking deltas.
func (h *FireworksHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	var inThinkBlock bool
	return h.inner.Stream(ctx, req, func(chunk StreamChunk) error {
		if chunk.Type == "delta" && chunk.TextDelta != "" && chunk.Thinking == "" {
			if inThinkBlock {
				if strings.Contains(chunk.TextDelta, "</think") {
					inThinkBlock = false
				}
				return callback(StreamChunk{Type: "delta", Thinking: chunk.TextDelta})
			}
			if strings.Contains(chunk.TextDelta, "<think") {
				inThinkBlock = true
				return callback(StreamChunk{Type: "delta", Thinking: chunk.TextDelta})
			}
		}
		if chunk.Type == "delta" && chunk.Thinking != "" {
			if strings.Contains(chunk.Thinking, "</think") {
				inThinkBlock = false
			} else if strings.Contains(chunk.Thinking, "<think") {
				inThinkBlock = true
			}
		}
		return callback(chunk)
	})
}

func (h *FireworksHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *FireworksHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.inner.Capabilities(), settings, thinking,
		InactiveInThinking("top_k", "min_p", "typical_p", "repetition_penalty", "seed"),
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
				valid := map[string]bool{"": true, "json_object": true, "json_schema": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'json_object', 'json_schema', or empty",
						Value: "",
					}
				}
			case "thinking_type":
				valid := map[string]bool{"": true, "enabled": true, "disabled": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'enabled', 'disabled', or empty",
						Value: "",
					}
				}
			case "thinking_keep":
				valid := map[string]bool{"": true, "last": true, "all": true, "none": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'last', 'all', 'none', or empty",
						Value: "",
					}
				}
			case "reasoning_effort":
				valid := map[string]bool{"": true, "none": true, "low": true, "medium": true, "high": true, "xhigh": true, "max": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be none/low/medium/high/xhigh/max",
						Value: "",
					}
				}
			case "reasoning_history":
				valid := map[string]bool{"": true, "disabled": true, "interleaved": true, "preserved": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'disabled', 'interleaved', 'preserved', or empty",
						Value: "",
					}
				}
			case "service_tier":
				if s, ok := val.(string); ok && s != "" && s != "priority" {
					return &SettingValidation{
						Error: "Must be 'priority' or empty",
						Value: "",
					}
				}
			case "context_length_exceeded_behavior":
				valid := map[string]bool{"": true, "truncate": true, "error": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'truncate', 'error', or empty",
						Value: "",
					}
				}
			}
			return nil
		}),
	)
}

var _ CapableHandler = (*FireworksHandler)(nil)
var _ SettingsValidator = (*FireworksHandler)(nil)

func (h *FireworksHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ ModelLister = (*FireworksHandler)(nil)
