package providers

import (
	"context"
	"strings"
)

// CerebrasHandler handles Cerebras API requests.
// Cerebras uses a text-only message format (no images/tool_calls in history).
// Qwen reasoning models emit <think/> tags that are tracked for reasoning extraction.
type CerebrasHandler struct {
	inner *openaiCompatHandler
}

func NewCerebrasHandler() *CerebrasHandler {
	const defaultModel = "zai-glm-4.7"
	return &CerebrasHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.cerebras.ai/v1",
			DefaultModel: defaultModel,
			NoStreamOptions: true,
			ExtraHeaders: map[string]string{
				"X-Cerebras-3rd-Party-Integration": "di-vrr",
			},
			Capabilities: &ProviderInfo{
				ID:           "cerebras",
				DefaultModel: defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:        false,
					SupportsReasoningEffort: false,
					SupportsTools:           true,
					SupportsImages:          false,
					SupportsPromptCache:     false,
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
						Default:     0,
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
				},
			},
			ModifyMessages: func(messages []map[string]interface{}, req *Request) []map[string]interface{} {
				return cerebrasConvertTextMessages(messages, req)
			},
		}),
	}
}

func (h *CerebrasHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	return h.inner.Send(ctx, req)
}

// Stream delegates to inner but wraps the callback to intercept <think/> tags
// for reasoning model tracking. Qwen reasoning models emit <think/> tags in
// content that need to be classified as Thinking deltas rather than TextDelta.
func (h *CerebrasHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	model := strings.ToLower(req.Provider.Model)
	if req.ModelOverride != "" {
		model = strings.ToLower(req.ModelOverride)
	}
	isReasoning := strings.Contains(model, "qwen")

	if !isReasoning {
		return h.inner.Stream(ctx, req, callback)
	}

	// Wrap callback for <think...>...</think...> tag tracking on reasoning models.
	// We track inThink state so content after </think...> is emitted as TextDelta.
	var buf strings.Builder
	inThink := false
	return h.inner.Stream(ctx, req, func(chunk StreamChunk) error {
		if chunk.Type != "delta" || chunk.TextDelta == "" || chunk.Thinking != "" {
			return callback(chunk)
		}
		buf.WriteString(chunk.TextDelta)
		text := buf.String()
		buf.Reset()

		for {
			if inThink {
				closeIdx := strings.Index(text, "</think")
				if closeIdx == -1 {
					// Still inside think block — emit as Thinking
					text = strings.TrimSpace(text)
					if text != "" {
						return callback(StreamChunk{Type: "delta", Thinking: text})
					}
					return nil
				}
				// Found closing tag — emit thinking up to it
				reasoning := strings.TrimSpace(text[:closeIdx])
				if reasoning != "" {
					if err := callback(StreamChunk{Type: "delta", Thinking: reasoning}); err != nil {
						return err
					}
				}
				// Advance past </think...>
				afterTag := closeIdx + len("</think")
				gtIdx := strings.IndexByte(text[afterTag:], '>')
				if gtIdx == -1 {
					// Incomplete close tag — nothing more to emit
					return nil
				}
				text = text[afterTag+gtIdx+1:]
				inThink = false
				continue
			}

			// Not in think block — look for opening tag
			openIdx := strings.Index(text, "<think")
			if openIdx == -1 {
				// No think tag — emit as plain text
				if text != "" {
					return callback(StreamChunk{Type: "delta", TextDelta: text})
				}
				return nil
			}
			// Emit text before the tag
			if openIdx > 0 {
				if err := callback(StreamChunk{Type: "delta", TextDelta: text[:openIdx]}); err != nil {
					return err
				}
			}
			// Advance past <think...>
			afterTag := openIdx + len("<think")
			gtIdx := strings.IndexByte(text[afterTag:], '>')
			if gtIdx == -1 {
				// Incomplete open tag — wait for more data
				buf.WriteString(text[openIdx:])
				return nil
			}
			text = text[afterTag+gtIdx+1:]
			inThink = true
		}
	})
}

func (h *CerebrasHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

func (h *CerebrasHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.inner.Capabilities(), settings, thinking)
}

var _ Handler = (*CerebrasHandler)(nil)
var _ CapableHandler = (*CerebrasHandler)(nil)
var _ SettingsValidator = (*CerebrasHandler)(nil)

func (h *CerebrasHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

var _ ModelLister = (*CerebrasHandler)(nil)

// cerebrasConvertTextMessages flattens OpenAI-format messages to text-only.
// Images in content arrays are replaced with "[Image content not supported in Cerebras]".
// For reasoning models (Qwen), <think/> tags are stripped from assistant messages.
func cerebrasConvertTextMessages(messages []map[string]interface{}, req *Request) []map[string]interface{} {
	isReasoning := strings.Contains(strings.ToLower(req.Provider.Model), "qwen")

	result := make([]map[string]interface{}, 0, len(messages))
	for _, msg := range messages {
		role, _ := msg["role"].(string)

		// Flatten content to a plain string
		var content string
		switch v := msg["content"].(type) {
		case string:
			content = v
		case []interface{}:
			var parts []string
			for _, item := range v {
				if part, ok := item.(map[string]interface{}); ok {
					switch part["type"] {
					case "text":
						if t, ok := part["text"].(string); ok {
							parts = append(parts, t)
						}
					case "image_url":
						parts = append(parts, "[Image content not supported in Cerebras]")
					}
				}
			}
			content = strings.Join(parts, "\n")
		}

		// Strip <think/> tags from assistant messages for reasoning models
		if role == "assistant" && isReasoning {
			content = stripThinkTags(content)
		}

		m := map[string]interface{}{
			"role": role,
		}
		if content != "" || role != "assistant" {
			m["content"] = content
		}
		// Preserve tool_calls, tool_call_id, etc. for non-text fields
		for k, v := range msg {
			if k != "content" && k != "role" {
				m[k] = v
			}
		}
		result = append(result, m)
	}
	return result
}

// stripThinkTags removes <think...>...</think...> blocks from content.
// Tags may include trailing spaces before the '>' (e.g. "<think >", "</think >").
func stripThinkTags(content string) string {
	for {
		start := strings.Index(content, "<think")
		if start == -1 {
			break
		}
		// Advance past the opening tag's '>'
		openClose := strings.IndexByte(content[start:], '>')
		if openClose == -1 {
			break
		}
		tagEnd := start + openClose + 1 // past '>'
		end := strings.Index(content[tagEnd:], "</think")
		if end == -1 {
			break
		}
		closeStart := tagEnd + end
		// Advance past the closing tag's '>'
		closeClose := strings.IndexByte(content[closeStart:], '>')
		if closeClose == -1 {
			break
		}
		content = content[:start] + content[closeStart+closeClose+1:]
	}
	return strings.TrimSpace(content)
}
