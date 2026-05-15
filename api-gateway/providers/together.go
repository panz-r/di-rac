package providers

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sort"
	"strings"
)

// TogetherHandler handles Together AI API requests.
// Wraps the shared OpenAI-compatible handler with Together-specific config:
//   - Base URL: https://api.together.xyz/v1
//   - 200+ models via /v1/models endpoint
//   - reasoning: {"enabled": true} for thinking mode
//   - reasoning_effort: low/medium/high
//   - context_length_exceeded_behavior: truncate/error
type TogetherHandler struct {
	inner *openaiCompatHandler
}

func NewTogetherHandler() *TogetherHandler {
	return &TogetherHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL: "https://api.together.xyz/v1",
			Capabilities: &ProviderInfo{
				ID:               "together",
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
						Key:         "top_k",
						Label:       "Top K",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(256),
						Step:        fPtr(1),
						Group:       "sampling",
						Description: "Limits sampling to top K tokens. 0 = disabled.",
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
						Description: "Minimum probability threshold. 0 = disabled.",
						ValidRange:  "0 – 1",
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
						Description: "Penalizes repetition (1 = disabled).",
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
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Group: "reasoning",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Description: "Controls reasoning depth for supported models.",
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
				model := req.Provider.Model
				if req.ModelOverride != "" {
					model = req.ModelOverride
				}
				if strings.Contains(model, "deepseek-reasoner") {
					messages = openaiAddReasoningContent(messages, req)
				}
				return messages
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				req.ApplySettingFloat(result, "temperature")

				if topP := req.SettingFloat("top_p"); topP > 0 {
					result["top_p"] = topP
				}

				if topK := req.SettingFloat("top_k"); topK > 0 {
					result["top_k"] = int(topK)
				}

				if minP := req.SettingFloat("min_p"); minP > 0 {
					result["min_p"] = minP
				}

				if rp := req.SettingFloat("repetition_penalty"); rp > 1 {
					result["repetition_penalty"] = rp
				}

				if seed := req.SettingFloat("seed"); seed > 0 {
					result["seed"] = int(seed)
				}

				if logprobs := req.SettingBool("logprobs"); logprobs {
					result["logprobs"] = true
					if topLogprobs := int(req.SettingFloat("top_logprobs")); topLogprobs > 0 {
						result["top_logprobs"] = topLogprobs
					}
				}

				if stop := req.SettingString("stop"); stop != "" {
					if seqs := splitStopSequences(stop); seqs != nil {
					result["stop"] = seqs
				}
				}

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				// Together-specific: reasoning parameter for thinking mode
				if req.Thinking != nil && req.Thinking.Type == "enabled" {
					result["reasoning"] = map[string]bool{"enabled": true}
				}

				if effort := req.SettingString("reasoning_effort"); effort != "" {
					result["reasoning_effort"] = effort
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

func (h *TogetherHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *TogetherHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, NewThinkTagStream(callback))
}

func (h *TogetherHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *TogetherHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
				valid := map[string]bool{"": true, "json_object": true, "json_schema": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'json_object', 'json_schema', or empty",
						Value: "",
					}
				}
			case "reasoning_effort":
				valid := map[string]bool{"": true, "low": true, "medium": true, "high": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'low', 'medium', 'high', or empty",
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

var _ Handler = (*TogetherHandler)(nil)
var _ CapableHandler = (*TogetherHandler)(nil)
var _ SettingsValidator = (*TogetherHandler)(nil)
var _ ModelLister = (*TogetherHandler)(nil)

// ListModels fetches models from Together's /v1/models endpoint.
// Together returns a flat JSON array, not the OpenAI {"data":[...]} wrapper,
// so we can't delegate to the shared fetchModelsHTTP.
func (h *TogetherHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := "https://api.together.xyz/v1"
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	url := strings.TrimRight(base, "/") + "/models"

	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return nil, err
	}
	if cfg.APIKey != "" {
		req.Header.Set("Authorization", "Bearer "+cfg.APIKey)
	}

	resp, err := SharedHTTPClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		errBody, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
		return nil, &ProviderAPIError{
			StatusCode: resp.StatusCode,
			Message:    fmt.Sprintf("/models returned status %d: %s", resp.StatusCode, string(errBody)),
			Retriable:  resp.StatusCode == 429 || resp.StatusCode >= 500,
		}
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, err
	}

	// Together returns a flat array, not {"data":[...]}.
	var raw []struct {
		ID            string `json:"id"`
		DisplayName   string `json:"display_name"`
		Type          string `json:"type"`
		ContextLength int    `json:"context_length"`
	}
	if err := json.Unmarshal(body, &raw); err != nil {
		return nil, err
	}

	entries := make([]ModelEntry, 0, len(raw))
	for _, m := range raw {
		if m.Type != "" && m.Type != "chat" {
			continue
		}
		name := m.DisplayName
		if name == "" {
			name = m.ID
		}
		entries = append(entries, ModelEntry{
			ID:            m.ID,
			Name:          name,
			ContextWindow: m.ContextLength,
		})
	}
	sort.Slice(entries, func(i, j int) bool {
		return entries[i].ID < entries[j].ID
	})
	return entries, nil
}
