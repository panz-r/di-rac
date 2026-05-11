package providers

import (
	"context"
	"math"
	"strings"
)

// MistralHandler handles Mistral API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with Mistral-specific config:
//   - tool_choice: configurable (auto, required, none)
//   - Content array support: delta.content can be string or [{type:text}] array
//   - Default model: mistral-large
//   - Mistral-specific: random_seed, top_k, response_format, stop
type MistralHandler struct {
	inner *openaiCompatHandler
}

func NewMistralHandler() *MistralHandler {
	return &MistralHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:             "https://api.mistral.ai/v1",
			DefaultModel:        "mistral-medium-latest",
			ToolChoice:          "auto",
			ContentArraySupport: true,
			Capabilities: &ProviderInfo{
				ID:           "mistral",
				MaxTokensDefault: 16384,
				DefaultModel: "mistral-medium-latest",
				Features: ProviderFeatures{
					SupportsThinking:  false,
					SupportsTools:     true,
					SupportsImages:    true,
					SupportsStreaming: true,
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
						Description: "Controls randomness (0 = deterministic, 1 = creative)",
						ValidRange:  "0 – 1",
					},
					{
						Key:        "top_p",
						Label:      "Top P",
						Type:       SettingSlider,
						Min:        fPtr(0),
						Max:        fPtr(1),
						Step:       fPtr(0.01),
						Default:    1.0,
						Group:      "sampling",
						ValidRange: "> 0 and ≤ 1",
					},
					{
						Key:         "top_k",
						Label:       "Top K",
						Type:        SettingSlider,
						Min:         fPtr(1),
						Max:         fPtr(100),
						Step:        fPtr(1),
						Group:       "sampling",
						Description: "Number of top tokens to sample from",
						ValidRange:  "1 – 100",
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
						Key:         "random_seed",
						Label:       "Random Seed",
						Type:        SettingNumber,
						Group:       "sampling",
						Description: "Set for deterministic sampling, 0 to disable",
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
						Description: "Force JSON output format",
					},
					{
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "output",
						Description: "Comma-separated stop sequences (max 16)",
					},
					{
						Key:   "tool_choice",
						Label: "Tool Choice",
						Type:  SettingSelect,
						Group: "tools",
						Options: []SelectOption{
							{Value: "auto", Label: "Auto"},
							{Value: "required", Label: "Required"},
							{Value: "none", Label: "None"},
						},
						Default:     "auto",
						Description: "Controls which tools the model may call",
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
				if p := req.SettingFloat("presence_penalty"); p != 0 {
					result["presence_penalty"] = p
				}
				if f := req.SettingFloat("frequency_penalty"); f != 0 {
					result["frequency_penalty"] = f
				}
				if topK := int(req.SettingFloat("top_k")); topK >= 1 {
					result["top_k"] = topK
				}
				if seed := req.SettingInt("random_seed"); seed != 0 {
					result["random_seed"] = seed
				}
				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}
				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = strings.Split(stop, ",")
				}
				if tc := req.SettingString("tool_choice"); tc != "" {
					result["tool_choice"] = tc
				}
			},
		}),
	}
}

func (h *MistralHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *MistralHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *MistralHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *MistralHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}

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

		// top_p must be > 0 (Mistral rejects 0)
		if s.Key == "top_p" {
			if num := toFloat(val); num <= 0 {
				v.Error = "Must be > 0"
				v.Value = float64(1)
			}
		}

		// top_k must be >= 1
		if s.Key == "top_k" {
			if num := toFloat(val); num < 1 {
				v.Error = "Must be >= 1"
				v.Value = float64(1)
			}
		}

		// response_format: only "json_object" is valid
		if s.Key == "response_format" {
			if rf, ok := val.(string); ok && rf != "" && rf != "json_object" {
				v.Error = "Must be 'json_object' or empty"
				v.Value = ""
			}
		}

		// tool_choice: validate options
		if s.Key == "tool_choice" {
			if tc, ok := val.(string); ok {
				valid := map[string]bool{"none": true, "auto": true, "required": true}
				if !valid[tc] {
					v.Error = "Must be 'none', 'auto', or 'required'"
					v.Value = "auto"
				}
			}
		}

		result.Settings[s.Key] = v
	}
	return result
}

var _ SettingsValidator = (*MistralHandler)(nil)

// ListModels delegates to the shared openaiCompatHandler model discovery.
func (h *MistralHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ ModelLister = (*MistralHandler)(nil)
