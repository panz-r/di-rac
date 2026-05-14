package providers

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"net/http"
	"strings"
)

// OpenCodeGoHandler handles OpenCode Go API requests.
// OpenCode Go is a subscription service for open-source coding models.
//   - Base URL: https://opencode.ai/zen/go/v1
//   - Model prefix: opencode-go/ (stripped before API calls)
//   - API: OpenAI-compatible (/chat/completions)
//   - Default model: deepseek-v4-flash
type OpenCodeGoHandler struct {
	inner *openaiCompatHandler
}

func NewOpenCodeGoHandler() *OpenCodeGoHandler {
	return &OpenCodeGoHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://opencode.ai/zen/go/v1",
			DefaultModel: "opencode-go/deepseek-v4-flash",
			Capabilities: &ProviderInfo{
				ID:           "opencode_go",
				DefaultModel: "opencode-go/deepseek-v4-flash",
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          false,
					SupportsStreaming:       true,
					SupportsPromptCache:     false,
				},
				Settings: []ProviderSetting{
						{
							Key:         "temperature",
							Label:       "Temperature",
							Type:        SettingSlider,
							Min:         fPtr(0),
							Max:         fPtr(1),
							Step:        fPtr(0.01),
							Default:     0.7,
							Group:       "sampling",
							Description: "Controls randomness (0 = deterministic, 1 = creative).",
							ValidRange:  "0 – 1",
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
							Key:         "stop",
							Label:       "Stop Sequences",
							Type:        SettingText,
							Group:       "sampling",
							Description: "Custom stop sequences (comma-separated, max 4).",
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
							Key:         "seed",
							Label:       "Seed",
							Type:        SettingNumber,
							Min:         fPtr(0),
							Group:       "sampling",
							Description: "Random seed for deterministic outputs.",
						},
						{
							Key:   "reasoning_effort",
							Label: "Reasoning Effort",
							Type:  SettingSelect,
							Scope: ScopePerMode,
							Group: "reasoning",
							Options: []SelectOption{
								{Value: "", Label: "Default"},
								{Value: "high", Label: "High"},
								{Value: "medium", Label: "Medium"},
								{Value: "low", Label: "Low"},
							},
							Description: "Controls reasoning depth (DeepSeek models).",
						},
						{
							Key:   "response_format",
							Label: "Response Format",
							Type:  SettingSelect,
							Group: "output",
							Options: []SelectOption{
								{Value: "", Label: "Default"},
								{Value: "json_object", Label: "JSON"},
							},
							Description: "Force JSON output format.",
						},
					},
				},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Strip "opencode-go/" prefix from model ID
				if m, ok := result["model"].(string); ok {
					m = strings.TrimPrefix(m, "opencode-go/")
					if m == "" {
						m = "deepseek-v4-flash"
					}
					result["model"] = m
				}

				if req.Thinking != nil && req.Thinking.Type == "enabled" {
					reasoningEffort := req.SettingString("reasoning_effort")
					if reasoningEffort == "" && req.Thinking.ReasoningEffort != "" {
						reasoningEffort = req.Thinking.ReasoningEffort
					}
					if reasoningEffort != "" {
						result["reasoning_effort"] = reasoningEffort
					}
					delete(result, "temperature")
					delete(result, "top_p")
				} else {
					if temperature := req.SettingFloat("temperature"); temperature != 0 {
						result["temperature"] = temperature
					}
					if topP := req.SettingFloat("top_p"); topP != 0 {
						result["top_p"] = topP
					}
				}

				if req.MaxTokens > 0 {
					result["max_tokens"] = req.MaxTokens
				}
				if stop := req.SettingString("stop"); stop != "" {
					if seqs := splitStopSequences(stop); seqs != nil {
					result["stop"] = seqs
				}
				}

				logprobs := req.SettingBool("logprobs")
				if !logprobs {
					logprobs = req.Logprobs
				}
				if logprobs {
					result["logprobs"] = true
					topLogprobs := int(req.SettingInt("top_logprobs"))
					if topLogprobs == 0 {
						topLogprobs = req.TopLogprobs
					}
					if topLogprobs > 0 {
						result["top_logprobs"] = topLogprobs
					}
				}

				if pp := req.SettingFloat("presence_penalty"); pp != 0 {
					result["presence_penalty"] = pp
				} else if req.PresencePenalty != 0 {
					result["presence_penalty"] = req.PresencePenalty
				}
				if fp := req.SettingFloat("frequency_penalty"); fp != 0 {
					result["frequency_penalty"] = fp
				} else if req.FrequencyPenalty != 0 {
					result["frequency_penalty"] = req.FrequencyPenalty
				}

				if seed := req.SettingInt("seed"); seed != 0 {
					result["seed"] = seed
				}
				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}
			},
		}),
	}
}

func (h *OpenCodeGoHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *OpenCodeGoHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *OpenCodeGoHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *OpenCodeGoHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}
	isThinking := thinking != nil && thinking.Type == "enabled"

	for _, s := range h.inner.Capabilities().Settings {
		val := settings[s.Key]
		v := SettingValidation{Status: StatusActive}

		if s.Type == SettingSlider {
			num := toFloat(val)
			if num == 0 {
				num = floatDefault(s.Default, 0)
			}
			clamped := num
			if s.Min != nil {
				clamped = math.Max(clamped, *s.Min)
			}
			if s.Max != nil {
				clamped = math.Min(clamped, *s.Max)
			}
			if clamped != num {
				v.Value = clamped
			}
			val = clamped
		}

		if isThinking {
			switch s.Key {
			case "temperature", "top_p", "presence_penalty", "frequency_penalty":
				v.Status = StatusInactive
				v.Message = "Ignored in thinking mode"
			}
		} else if s.Key == "reasoning_effort" {
			v.Status = StatusInactive
			v.Message = "Only applies in thinking mode"
		}

		if s.Key == "stop" {
			if stop, ok := val.(string); ok && stop != "" {
				seqs := splitStopSequences(stop)
				if len(seqs) > 4 {
					v.Error = "Max 4 stop sequences"
					v.Value = strings.Join(seqs[:4], ",")
				}
			}
		}

		if s.Key == "top_logprobs" {
			logprobsEnabled, _ := settings["logprobs"].(bool)
			if logprobsEnabled {
				num := toFloat(val)
				if num <= 0 {
					v.Error = "Must be > 0 when logprobs is enabled"
					v.Value = float64(1)
				}
			}
		}

		result.Settings[s.Key] = v
	}
	return result
}

// ListModels fetches available models from the OpenCode Go API.
func (h *OpenCodeGoHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	client := SharedHTTPClient
	req, err := http.NewRequestWithContext(ctx, "GET", "https://opencode.ai/zen/go/v1/models", nil)
	if err != nil {
		return nil, err
	}
	if cfg.APIKey != "" {
		req.Header.Set("Authorization", "Bearer "+cfg.APIKey)
	}

	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		errBody, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
		return nil, fmt.Errorf("OpenCode Go /models returned status %d: %s", resp.StatusCode, string(errBody))
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, err
	}

	var result struct {
		Data []struct {
			ID            string `json:"id"`
			Name          string `json:"name"`
			ContextLength *int   `json:"context_length"`
		} `json:"data"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, err
	}

	entries := make([]ModelEntry, 0, len(result.Data))
	for _, m := range result.Data {
		entry := ModelEntry{
			ID:   "opencode-go/" + m.ID,
			Name: m.Name,
		}
		if m.ContextLength != nil {
			entry.ContextWindow = *m.ContextLength
		}
		entries = append(entries, entry)
	}
	return entries, nil
}

var _ Handler = (*OpenCodeGoHandler)(nil)
var _ CapableHandler = (*OpenCodeGoHandler)(nil)
var _ SettingsValidator = (*OpenCodeGoHandler)(nil)
var _ ModelLister = (*OpenCodeGoHandler)(nil)
