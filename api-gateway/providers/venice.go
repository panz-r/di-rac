package providers

import (
	"context"
	"strings"
)

// VeniceHandler handles Venice AI API requests.
// Venice AI is a privacy-first, OpenAI-compatible inference platform.
//   - Base URL: https://api.venice.ai/api/v1
//   - Model format: Raw model IDs (e.g., "venice-uncensored", "grok-4.3")
//   - Venice-specific: venice_parameters for web search, reasoning control, etc.
//   - Zero data retention (privacy-first)
type VeniceHandler struct {
	inner *openaiCompatHandler
}

func NewVeniceHandler() *VeniceHandler {
	const defaultModel = "venice-uncensored"
	return &VeniceHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.venice.ai/api/v1",
			DefaultModel: defaultModel,
			Capabilities: &ProviderInfo{
				ID:               "venice",
				DefaultModel:     defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsTools:       true,
					SupportsImages:      true,
					SupportsStreaming:   true,
					SupportsPromptCache: true,
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
							{Value: "text", Label: "Text"},
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
							{Value: "none", Label: "None"},
							{Value: "low", Label: "Low"},
							{Value: "medium", Label: "Medium"},
							{Value: "high", Label: "High"},
						},
						Description: "Controls reasoning depth for reasoning models.",
					},
					// Venice-specific: reasoning control
					{
						Key:         "strip_thinking_response",
						Label:       "Strip Thinking Response",
						Type:        SettingToggle,
						Group:       "venice",
						Description: "Strip thinking blocks from the response.",
					},
					{
						Key:         "disable_thinking",
						Label:       "Disable Thinking",
						Type:        SettingToggle,
						Group:       "venice",
						Description: "Disable thinking for reasoning models.",
					},
					// Venice-specific: web search
					{
						Key:   "enable_web_search",
						Label: "Web Search",
						Type:  SettingSelect,
						Group: "venice",
						Options: []SelectOption{
							{Value: "", Label: "Default (off)"},
							{Value: "off", Label: "Off"},
							{Value: "on", Label: "On"},
							{Value: "auto", Label: "Auto"},
						},
						Description: "Enable web search for real-time information.",
					},
					{
						Key:         "enable_web_scraping",
						Label:       "Web Scraping",
						Type:        SettingToggle,
						Group:       "venice",
						Description: "Scrape up to 5 URLs from user message for context.",
					},
					{
						Key:         "enable_web_citations",
						Label:       "Web Citations",
						Type:        SettingToggle,
						Group:       "venice",
						Description: "Request [REF] citations for web search results.",
					},
					{
						Key:         "enable_x_search",
						Label:       "xAI Search",
						Type:        SettingToggle,
						Group:       "venice",
						Description: "Enable xAI native search (for Grok models).",
					},
					// Venice-specific: other
					{
						Key:         "include_venice_system_prompt",
						Label:       "Include Venice System Prompt",
						Type:        SettingToggle,
						Group:       "venice",
						Default:     true,
						Description: "Include Venice's default system prompts.",
					},
					{
						Key:         "character_slug",
						Label:       "Character Slug",
						Type:        SettingText,
						Group:       "venice",
						Description: "Public Venice character ID.",
					},
					{
						Key:         "prompt_cache_key",
						Label:       "Prompt Cache Key",
						Type:        SettingText,
						Group:       "venice",
						Description: "Routing hint to improve cache hit rates.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				if req.SettingIsNull("temperature") {
					delete(result, "temperature")
				} else {
					result["temperature"] = req.SettingFloat("temperature")
				}

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
					result["stop"] = splitStopSequences(stop)
				}

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				if effort := req.SettingString("reasoning_effort"); effort != "" {
					result["reasoning_effort"] = effort
				} else if req.Thinking != nil && req.Thinking.ReasoningEffort != "" {
					result["reasoning_effort"] = req.Thinking.ReasoningEffort
				}

				// Venice-specific: venice_parameters
				vp := make(map[string]interface{})
				if req.SettingBool("strip_thinking_response") {
					vp["strip_thinking_response"] = true
				}
				if req.SettingBool("disable_thinking") {
					vp["disable_thinking"] = true
				}
				if ws := req.SettingString("enable_web_search"); ws != "" {
					vp["enable_web_search"] = ws
				}
				if req.SettingBool("enable_web_scraping") {
					vp["enable_web_scraping"] = true
				}
				if req.SettingBool("enable_web_citations") {
					vp["enable_web_citations"] = true
				}
				if req.SettingBool("enable_x_search") {
					vp["enable_x_search"] = true
				}
				if val, ok := req.Settings["include_venice_system_prompt"]; ok {
					if b, _ := val.(bool); !b {
						vp["include_venice_system_prompt"] = false
					}
				}
				if slug := req.SettingString("character_slug"); slug != "" {
					vp["character_slug"] = slug
				}
				if key := req.SettingString("prompt_cache_key"); key != "" {
					vp["prompt_cache_key"] = key
				}
				if len(vp) > 0 {
					result["venice_parameters"] = vp
				}
			},
		}),
	}
}

func (h *VeniceHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *VeniceHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *VeniceHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *VeniceHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
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
			case "enable_web_search":
				valid := map[string]bool{"": true, "off": true, "on": true, "auto": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'off', 'on', 'auto', or empty",
						Value: "",
					}
				}
			case "response_format":
				valid := map[string]bool{"": true, "json_object": true, "text": true}
				if s, ok := val.(string); ok && !valid[s] {
					return &SettingValidation{
						Error: "Must be 'json_object', 'text', or empty",
						Value: "",
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

var _ Handler = (*VeniceHandler)(nil)
var _ CapableHandler = (*VeniceHandler)(nil)
var _ SettingsValidator = (*VeniceHandler)(nil)
var _ ModelLister = (*VeniceHandler)(nil)

func (h *VeniceHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}
