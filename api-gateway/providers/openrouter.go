package providers

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"net/http"
	"sort"
	"strings"
)

// OpenRouterHandler handles OpenRouter API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with OpenRouter-specific config:
//   - Custom headers (HTTP-Referer, X-Title) for app attribution
//   - Full OpenRouter parameter support: plugins, provider routing, service tier,
//     session tracking, cache control, trace metadata, verbosity, modalities,
//     parallel tool calls, advanced sampling (min_p, top_a, top_k, repetition_penalty)
//   - Default model: anthropic/claude-sonnet-4.5
type OpenRouterHandler struct {
	inner *openaiCompatHandler
}

func NewOpenRouterHandler() *OpenRouterHandler {
	return &OpenRouterHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://openrouter.ai/api/v1",
			DefaultModel: "anthropic/claude-sonnet-4.5",
			ExtraHeaders: map[string]string{
				"HTTP-Referer": "https://dirac.run",
				"X-Title":      "Dirac",
			},
			Capabilities: &ProviderInfo{
				ID:           "openrouter",
				DefaultModel: "anthropic/claude-sonnet-4.5",
				Features: ProviderFeatures{
					SupportsThinking:        true,
					SupportsReasoningEffort: true,
					SupportsTools:           true,
					SupportsImages:          true,
					SupportsStreaming:       true,
					SupportsPromptCache:     true,
				},
				Settings: []ProviderSetting{
					// --- sampling ---
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
						Key:         "min_p",
						Label:       "Min P",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Group:       "sampling",
						Description: "Minimum probability threshold for token selection.",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "top_k",
						Label:       "Top K",
						Type:        SettingSlider,
						Min:         fPtr(1),
						Max:         fPtr(100),
						Step:        fPtr(1),
						Group:       "sampling",
						Description: "Consider only top K tokens at each step.",
						ValidRange:  "1 – 100",
					},
					{
						Key:         "top_a",
						Label:       "Top A",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Group:       "sampling",
						Description: "Dynamic Top-P alternative based on probability.",
						ValidRange:  "0 – 1",
					},
					{
						Key:        "presence_penalty",
						Label:      "Presence Penalty",
						Type:       SettingSlider,
						Min:        fPtr(-2),
						Max:        fPtr(2),
						Step:       fPtr(0.1),
						Group:      "sampling",
						ValidRange: "-2 – 2",
					},
					{
						Key:        "frequency_penalty",
						Label:      "Frequency Penalty",
						Type:       SettingSlider,
						Min:        fPtr(-2),
						Max:        fPtr(2),
						Step:       fPtr(0.1),
						Group:      "sampling",
						ValidRange: "-2 – 2",
					},
					{
						Key:         "repetition_penalty",
						Label:       "Repetition Penalty",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(2),
						Step:        fPtr(0.1),
						Group:       "sampling",
						Description: "OpenRouter-specific penalty for repeated tokens.",
						ValidRange:  "0 – 2",
					},
					{
						Key:         "max_completion_tokens",
						Label:       "Max Completion Tokens",
						Type:        SettingNumber,
						Min:         fPtr(1),
						Group:       "sampling",
						Description: "Maximum tokens in completion (preferred over max_tokens).",
					},
					{
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "sampling",
						Description: "Custom stop sequences (comma-separated, max 4).",
					},
					{
						Key:         "logprobs",
						Label:       "Log Probabilities",
						Type:        SettingToggle,
						Group:       "sampling",
						Description: "Return log probabilities of output tokens.",
					},
					{
						Key:        "top_logprobs",
						Label:      "Top Logprobs",
						Type:       SettingSlider,
						Min:        fPtr(0),
						Max:        fPtr(20),
						Step:       fPtr(1),
						Group:      "sampling",
						ValidRange: "0 – 20",
					},
					// --- reasoning ---
					{
						Key:   "reasoning_effort",
						Label: "Reasoning Effort",
						Type:  SettingSelect,
						Scope: ScopePerMode,
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "high", Label: "High"},
							{Value: "medium", Label: "Medium"},
							{Value: "low", Label: "Low"},
						},
						Group:       "reasoning",
						Description: "Controls reasoning depth for supported models. Only applies in thinking mode.",
					},
					// --- output ---
					{
						Key:   "response_format",
						Label: "Response Format",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "json_object", Label: "JSON"},
							{Value: "json_schema", Label: "JSON Schema"},
						},
						Description: "Force structured output format.",
					},
					{
						Key:   "verbosity",
						Label: "Verbosity",
						Type:  SettingSelect,
						Group: "output",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
							{Value: "xhigh", Label: "XHigh"},
							{Value: "max", Label: "Max"},
						},
						Description: "Control response detail level (Anthropic models only).",
					},
					{
						Key:         "modalities",
						Label:       "Modalities",
						Type:        SettingText,
						Group:       "output",
						Description: "Output modalities (comma-separated): text, image, audio.",
					},
					// --- tools ---
					{
						Key:         "parallel_tool_calls",
						Label:       "Parallel Tool Calls",
						Type:        SettingToggle,
						Default:     true,
						Group:       "tools",
						Description: "Allow the model to call multiple tools in parallel.",
					},
					// --- routing ---
					{
						Key:   "provider",
						Label: "Provider",
						Type:  SettingSelect,
						Group: "routing",
						Options: []SelectOption{
							{Value: "", Label: "Auto"},
							{Value: "openai", Label: "OpenAI"},
							{Value: "anthropic", Label: "Anthropic"},
							{Value: "google", Label: "Google"},
							{Value: "mistral", Label: "Mistral"},
							{Value: "meta", Label: "Meta"},
							{Value: "deepseek", Label: "DeepSeek"},
							{Value: "perplexity", Label: "Perplexity"},
						},
						Description: "Preferred upstream provider for model routing.",
					},
					{
						Key:   "service_tier",
						Label: "Service Tier",
						Type:  SettingSelect,
						Group: "routing",
						Options: []SelectOption{
							{Value: "", Label: "Default"},
							{Value: "default", Label: "Default"},
							{Value: "flex", Label: "Flex"},
						},
						Description: "Service tier (flex is cheaper but may queue).",
					},
					{
						Key:         "plugins",
						Label:       "Plugins",
						Type:        SettingText,
						Group:       "routing",
						Description: "Comma-separated OpenRouter plugin IDs: web, file-parser, response-healing, context-compression.",
					},
					{
						Key:         "session_id",
						Label:       "Session ID",
						Type:        SettingText,
						Scope:       ScopeGlobal,
						Group:       "routing",
						Description: "Persistent session ID for request grouping (max 256 chars).",
					},
					{
						Key:         "cache_control",
						Label:       "Prompt Caching",
						Type:        SettingToggle,
						Group:       "routing",
						Description: "Enable prompt caching (Anthropic models only).",
					},
					{
						Key:         "trace_id",
						Label:       "Trace ID",
						Type:        SettingText,
						Group:       "routing",
						Description: "Trace ID for observability.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				isThinking := req.Thinking != nil && req.Thinking.Type == "enabled"

				if isThinking {
					effort := req.SettingString("reasoning_effort")
					if effort == "" && req.Thinking.ReasoningEffort != "" {
						effort = req.Thinking.ReasoningEffort
					}
					if effort != "" {
						result["reasoning_effort"] = effort
					}
					delete(result, "temperature")
					delete(result, "top_p")
				} else {
					result["temperature"] = req.SettingFloat("temperature")
					tp := req.SettingFloat("top_p")
					if tp == 0 {
						tp = 1.0
					}
					result["top_p"] = tp
				}

				// Advanced sampling parameters
				if minP := req.SettingFloat("min_p"); minP > 0 {
					result["min_p"] = minP
				}
				if topK := int(req.SettingFloat("top_k")); topK > 0 {
					result["top_k"] = topK
				}
				if topA := req.SettingFloat("top_a"); topA > 0 {
					result["top_a"] = topA
				}

				// Presence/frequency penalties with typed field fallback
				pp := req.SettingFloat("presence_penalty")
				if pp == 0 {
					pp = req.PresencePenalty
				}
				if pp != 0 {
					result["presence_penalty"] = pp
				}
				fp := req.SettingFloat("frequency_penalty")
				if fp == 0 {
					fp = req.FrequencyPenalty
				}
				if fp != 0 {
					result["frequency_penalty"] = fp
				}
				if rp := req.SettingFloat("repetition_penalty"); rp > 0 {
					result["repetition_penalty"] = rp
				}

				// Max completion tokens (preferred over max_tokens)
				if mct := int(req.SettingFloat("max_completion_tokens")); mct > 0 {
					result["max_completion_tokens"] = mct
				}

				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = strings.Split(stop, ",")
				}

				// Logprobs with typed field fallback
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

				// Response format
				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				// Verbosity
				if v := req.SettingString("verbosity"); v != "" {
					result["verbosity"] = v
				}

				// Modalities
				if mod := req.SettingString("modalities"); mod != "" {
					result["modalities"] = strings.Split(mod, ",")
				}

				// Parallel tool calls
				if ptc := req.SettingBool("parallel_tool_calls"); ptc {
					result["parallel_tool_calls"] = true
				}

				// Provider routing
				if p := req.SettingString("provider"); p != "" {
					result["provider"] = map[string]string{"name": p}
				}

				// Service tier
				if st := req.SettingString("service_tier"); st != "" {
					result["service_tier"] = st
				}

				// Plugins: convert comma-separated to [{id: "web"}] format
				if plugins := req.SettingString("plugins"); plugins != "" {
					parts := strings.Split(plugins, ",")
					arr := make([]map[string]interface{}, 0, len(parts))
					for _, p := range parts {
						p = strings.TrimSpace(p)
						if p != "" {
							arr = append(arr, map[string]interface{}{"id": p})
						}
					}
					if len(arr) > 0 {
						result["plugins"] = arr
					}
				}

				// Session tracking
				if sid := req.SettingString("session_id"); sid != "" {
					result["session_id"] = sid
				}

				// Cache control
				if req.SettingBool("cache_control") {
					result["cache_control"] = map[string]string{"type": "ephemeral"}
				}

				// Trace metadata
				if tid := req.SettingString("trace_id"); tid != "" {
					result["trace"] = map[string]string{"trace_id": tid}
				}
			},
		}),
	}
}

func (h *OpenRouterHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *OpenRouterHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *OpenRouterHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *OpenRouterHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}
	isThinking := thinking != nil && thinking.Type == "enabled"

	validProviders := map[string]bool{
		"openai": true, "anthropic": true, "google": true,
		"mistral": true, "meta": true, "deepseek": true, "perplexity": true,
	}
	validPlugins := map[string]bool{
		"web": true, "file-parser": true, "response-healing": true, "context-compression": true,
	}
	validModalities := map[string]bool{"text": true, "image": true, "audio": true}
	validVerbosity := map[string]bool{"low": true, "medium": true, "high": true, "xhigh": true, "max": true}

	for _, s := range h.inner.Capabilities().Settings {
		val := settings[s.Key]
		v := SettingValidation{Status: StatusActive}

		// Clamp slider values to [min, max]
		if s.Type == SettingSlider {
			num := toFloat(val)
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

		// Active/inactive based on thinking mode
		if isThinking {
			switch s.Key {
			case "temperature", "top_p", "presence_penalty", "frequency_penalty",
				"repetition_penalty", "min_p", "top_a", "top_k":
				v.Status = StatusInactive
				v.Message = "Ignored in thinking mode"
			}
		} else if s.Key == "reasoning_effort" {
			v.Status = StatusInactive
			v.Message = "Only applies in thinking mode"
		}

		switch s.Key {
		case "response_format":
			if rf, ok := val.(string); ok && rf != "" && rf != "json_object" && rf != "json_schema" {
				v.Error = "Must be 'json_object', 'json_schema', or empty"
				v.Value = ""
			}
		case "service_tier":
			if st, ok := val.(string); ok && st != "" && st != "default" && st != "flex" {
				v.Error = "Must be 'default', 'flex', or empty"
				v.Value = ""
			}
		case "provider":
			if p, ok := val.(string); ok && p != "" && !validProviders[p] {
				v.Error = "Unknown provider: " + p
				v.Value = ""
			}
		case "verbosity":
			if vv, ok := val.(string); ok && vv != "" && !validVerbosity[vv] {
				v.Error = "Must be 'low', 'medium', 'high', 'xhigh', or 'max'"
				v.Value = ""
			}
		case "plugins":
			if plugins, ok := val.(string); ok && plugins != "" {
				for _, p := range strings.Split(plugins, ",") {
					p = strings.TrimSpace(p)
					if p != "" && !validPlugins[p] {
						v.Error = fmt.Sprintf("Invalid plugin: %s", p)
						break
					}
				}
			}
		case "modalities":
			if mod, ok := val.(string); ok && mod != "" {
				for _, m := range strings.Split(mod, ",") {
					m = strings.TrimSpace(m)
					if m != "" && !validModalities[m] {
						v.Error = fmt.Sprintf("Invalid modality: %s", m)
						break
					}
				}
			}
		case "session_id":
			if sid, ok := val.(string); ok && len(sid) > 256 {
				v.Error = "Must be ≤ 256 characters"
				v.Value = sid[:256]
			}
		case "top_logprobs":
			if toFloat(settings["logprobs"]) != 0 {
				num := toFloat(val)
				if num <= 0 {
					v.Error = "Must be > 0 when logprobs is enabled"
					v.Value = float64(1)
				}
			}
		case "stop":
			if stop, ok := val.(string); ok && stop != "" {
				seqs := strings.Split(stop, ",")
				if len(seqs) > 4 {
					v.Error = "Max 4 stop sequences"
					v.Value = strings.Join(seqs[:4], ",")
				}
			}
		}

		result.Settings[s.Key] = v
	}
	return result
}

// ListModels fetches the available models from the OpenRouter API.
func (h *OpenRouterHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	baseURL := "https://openrouter.ai/api/v1"
	if cfg.BaseURL != "" {
		baseURL = cfg.BaseURL
	}

	client := SharedHTTPClient
	req, err := http.NewRequestWithContext(ctx, "GET", baseURL+"/models", nil)
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
		return nil, fmt.Errorf("OpenRouter /models returned status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}

	var result struct {
		Data []openRouterRawModel `json:"data"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, err
	}

	entries := make([]ModelEntry, 0, len(result.Data))
	for _, m := range result.Data {
		entry := ModelEntry{
			ID:             m.ID,
			Name:           m.Name,
			Description:    ptrStr(m.Description),
			ContextWindow:  ptrInt(m.ContextLength),
			MaxTokens:      ptrInt(m.TopProvider.MaxCompletionTokens),
			SupportsImages: ptrBool(m.Architecture.InputModalities, "image"),
		}

		for _, p := range m.SupportedParameters {
			if p == "include_reasoning" || p == "reasoning" {
				entry.SupportsThinking = true
				entry.ThinkingMaxBudget = 32768
				break
			}
		}

		entries = append(entries, entry)
	}

	sort.Slice(entries, func(i, j int) bool {
		return entries[i].ID < entries[j].ID
	})

	return entries, nil
}

type openRouterRawModel struct {
	ID            string  `json:"id"`
	Name          string  `json:"name"`
	Description   *string `json:"description"`
	ContextLength *int    `json:"context_length"`
	TopProvider   struct {
		MaxCompletionTokens *int `json:"max_completion_tokens"`
	} `json:"top_provider"`
	Architecture struct {
		InputModalities []string `json:"input_modalities"`
	} `json:"architecture"`
	SupportedParameters []string `json:"supported_parameters"`
}

func ptrStr(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

func ptrInt(p *int) int {
	if p == nil {
		return 0
	}
	return *p
}

func ptrBool(slice []string, target string) bool {
	for _, s := range slice {
		if s == target {
			return true
		}
	}
	return false
}

var _ SettingsValidator = (*OpenRouterHandler)(nil)
var _ ModelLister = (*OpenRouterHandler)(nil)
