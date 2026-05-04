package providers

import (
	"context"
	"math"
	"strings"
)

// NvidiaNimHandler handles NVIDIA NIM API requests via their OpenAI-compatible endpoint.
// Wraps the shared openaiCompatHandler with NIM-specific config:
//   - chat_template_kwargs for thinking mode
//   - parallel_tool_calls for concurrent tool use
//   - Standard OpenAI parameters (temperature, top_p, stop, logprobs, etc.)
//   - Default model: nvidia/llama-3.1-nemotron-ultra-253b-v1
type NvidiaNimHandler struct {
	inner *openaiCompatHandler
}

func NewNvidiaNimHandler() *NvidiaNimHandler {
	return &NvidiaNimHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://integrate.api.nvidia.com/v1",
			DefaultModel: "nvidia/llama-3.1-nemotron-ultra-253b-v1",
			Capabilities: &ProviderInfo{
				ID:           "nvidia_nim",
				DefaultModel: "nvidia/llama-3.1-nemotron-ultra-253b-v1",
				Features: ProviderFeatures{
					SupportsThinking:    true,
					SupportsTools:       true,
					SupportsImages:      false,
					SupportsStreaming:   true,
					SupportsPromptCache: false,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     1.0,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 1 = creative).",
						ValidRange:  "0 – 1",
					},
					{
						Key:         "top_p",
						Label:       "Top P",
						Type:        SettingSlider,
						Min:         fPtr(0.01),
						Max:         fPtr(1),
						Step:        fPtr(0.01),
						Default:     0.95,
						Group:       "sampling",
						Description: "Nucleus sampling threshold.",
						ValidRange:  "0.01 – 1",
					},
					{
						Key:         "stop",
						Label:       "Stop Sequences",
						Type:        SettingText,
						Group:       "sampling",
						Description: "Custom stop sequences (comma-separated).",
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
					{
						Key:         "parallel_tool_calls",
						Label:       "Parallel Tool Calls",
						Type:        SettingToggle,
						Group:       "tools",
						Description: "Allow the model to call multiple tools in parallel.",
					},
				},
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				// Thinking mode: translate to NIM's chat_template_kwargs
				if req.Thinking != nil && req.Thinking.Type == "enabled" {
					result["chat_template_kwargs"] = map[string]interface{}{"enable_thinking": true}
					delete(result, "temperature")
					delete(result, "top_p")
				} else {
					result["temperature"] = req.SettingFloat("temperature")
					tp := req.SettingFloat("top_p")
					if tp == 0 {
						tp = 0.95
					}
					result["top_p"] = tp
				}

				if stop := req.SettingString("stop"); stop != "" {
					result["stop"] = strings.Split(stop, ",")
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

				if rf := req.SettingString("response_format"); rf != "" {
					result["response_format"] = map[string]string{"type": rf}
				}

				if req.SettingBool("parallel_tool_calls") {
					result["parallel_tool_calls"] = true
				}
			},
		}),
	}
}

func (h *NvidiaNimHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

func (h *NvidiaNimHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	return h.inner.Stream(ctx, req, callback)
}

func (h *NvidiaNimHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *NvidiaNimHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}
	isThinking := thinking != nil && thinking.Type == "enabled"

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
			case "temperature", "top_p":
				v.Status = StatusInactive
				v.Message = "Ignored in thinking mode"
			}
		}

		// Cross-parameter: logprobs requires top_logprobs > 0
		if s.Key == "top_logprobs" && toFloat(settings["logprobs"]) != 0 {
			num := toFloat(val)
			if num <= 0 {
				v.Error = "Must be > 0 when logprobs is enabled"
				v.Value = float64(1)
			}
		}

		result.Settings[s.Key] = v
	}
	return result
}

var _ SettingsValidator = (*NvidiaNimHandler)(nil)
