package providers

import (
	"context"
	"strings"
)

// QwenHandler handles Qwen (Alibaba Cloud) API requests.
// Wraps the shared openaiCompatHandler with Qwen-specific config:
//   - Dynamic base URL based on apiLine (china/international)
//   - R1 format for DeepSeek Reasoner and Qwen3 reasoning models (merge consecutive same-role messages)
//   - enable_thinking / thinking_budget for Qwen3 reasoning models
//   - max_completion_tokens (not max_tokens)
//   - prompt_cache_hit_tokens / prompt_cache_miss_tokens in usage
type QwenHandler struct {
	inner   *openaiCompatHandler
	apiLine string // "china" or "international"
}

func NewQwenHandler() *QwenHandler {
	const defaultModel = "qwen3-235b-a22b"
	return &QwenHandler{
		apiLine: "china",
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://dashscope.aliyuncs.com/compatible-mode/v1",
			DefaultModel:        defaultModel,
			MaxCompletionTokens: true,
			Capabilities: &ProviderInfo{
				ID:           "qwen",
				DefaultModel: defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:    true,
					SupportsTools:       true,
					SupportsImages:      true,
					SupportsPromptCache: false,
					SupportsStreaming:   true,
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
						Key:         "enable_thinking",
						Label:       "Enable Thinking",
						Type:        SettingToggle,
						Group:       "reasoning",
						Description: "Enable thinking/reasoning for Qwen3 and R1 models.",
					},
					{
						Key:         "thinking_budget",
						Label:       "Thinking Budget",
						Type:        SettingNumber,
						Min:         fPtr(1),
						Group:       "reasoning",
						Description: "Token budget for thinking (only applies when thinking is enabled).",
					},
				},
			},
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				model := req.Provider.Model
				if req.ModelOverride != "" {
					model = req.ModelOverride
				}
				isDeepseekReasoner := strings.Contains(model, "deepseek-r1")
				isReasoningFamily := strings.Contains(model, "qwen3") || model == "qwen-plus-latest" || model == "qwen-turbo-latest"
				thinkingOn := req.Thinking != nil && req.Thinking.BudgetTokens > 0

				if isDeepseekReasoner || (thinkingOn && isReasoningFamily) {
					messages = openaiAddReasoningContent(messages, req)
					messages = qwenMergeConsecutiveRoles(messages)
				}
				return messages
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				model, _ := result["model"].(string)
				isDeepseekReasoner := strings.Contains(model, "deepseek-r1")
				isReasoningFamily := strings.Contains(model, "qwen3") || model == "qwen-plus-latest" || model == "qwen-turbo-latest"
				thinkingOn := req.Thinking != nil && req.Thinking.BudgetTokens > 0

				// Temperature: omit when reasoning
				if isDeepseekReasoner || (thinkingOn && isReasoningFamily) {
					delete(result, "temperature")
				}

				// Thinking params for Qwen3 family
				if isReasoningFamily {
					if thinkingOn {
						result["enable_thinking"] = true
						result["thinking_budget"] = req.Thinking.BudgetTokens
					} else {
						result["enable_thinking"] = false
					}
				}
			},
		}),
	}
}

func (h *QwenHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *QwenHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *QwenHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *QwenHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.inner.config.BaseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", cfg.APIKey)
}

func (h *QwenHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.inner.Capabilities(), settings, thinking)
}

var _ CapableHandler = (*QwenHandler)(nil)
var _ SettingsValidator = (*QwenHandler)(nil)
var _ ModelLister = (*QwenHandler)(nil)

// qwenCanMerge reports whether a message is a plain user/assistant message safe to merge.
// Tool messages (tool_calls, tool_call_id) must never be merged.
func qwenCanMerge(m map[string]interface{}) bool {
	role, _ := m["role"].(string)
	if role != "user" && role != "assistant" {
		return false
	}
	if _, ok := m["tool_calls"]; ok {
		return false
	}
	if _, ok := m["tool_call_id"]; ok {
		return false
	}
	return true
}

// qwenMergeConsecutiveRoles merges consecutive plain user/assistant messages with the same role.
// Tool messages and messages with tool_calls/tool_call_id are never merged.
func qwenMergeConsecutiveRoles(messages []map[string]interface{}) []map[string]interface{} {
	var merged []map[string]interface{}

	for _, msg := range messages {
		if len(merged) == 0 {
			merged = append(merged, msg)
			continue
		}

		last := merged[len(merged)-1]
		lastRole, _ := last["role"].(string)
		curRole, _ := msg["role"].(string)

		if lastRole == curRole && qwenCanMerge(last) && qwenCanMerge(msg) {
			lastContent := qwenContentToString(last["content"])
			curContent := qwenContentToString(msg["content"])
			if lastContent != "" && curContent != "" {
				last["content"] = lastContent + "\n" + curContent
			} else if curContent != "" {
				last["content"] = curContent
			}
		} else {
			merged = append(merged, msg)
		}
	}

	return merged
}

// qwenContentToString extracts a string from content that may be a string or array.
func qwenContentToString(content interface{}) string {
	switch v := content.(type) {
	case string:
		return v
	case []interface{}:
		var parts []string
		for _, item := range v {
			if m, ok := item.(map[string]interface{}); ok {
				if text, _ := m["text"].(string); text != "" {
					parts = append(parts, text)
				}
			}
		}
		return strings.Join(parts, "\n")
	default:
		return ""
	}
}
