package providers

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
)

// parseToolInput parses a JSON string into a value suitable for Anthropic tool input.
// Returns an empty map on failure so the API call does not break.
func parseToolInput(args string) interface{} {
	var input interface{}
	if args != "" {
		_ = json.Unmarshal([]byte(args), &input)
	}
	if input == nil {
		input = map[string]interface{}{}
	}
	return input
}

// AnthropicHandler handles Anthropic API requests via direct HTTP.
type AnthropicHandler struct{}

func NewAnthropicHandler() *AnthropicHandler {
	return &AnthropicHandler{}
}

func NewAnthropicHandlerWithKey(apiKey string) *AnthropicHandler {
	return &AnthropicHandler{}
}

func (h *AnthropicHandler) resolveConfig(req *Request) (baseURL, apiKey string) {
	baseURL = "https://api.anthropic.com"
	if req.Provider.BaseURL != "" {
		baseURL = req.Provider.BaseURL
	}
	apiKey = req.Provider.APIKey
	return
}

func (h *AnthropicHandler) buildRequestBody(req *Request) map[string]interface{} {
	var messages []map[string]interface{}

	for _, msg := range req.Messages {
		var content []map[string]interface{}

		if len(msg.ContentBlocks) > 0 {
			for _, block := range msg.ContentBlocks {
				switch block.Type {
				case "text":
					content = append(content, map[string]interface{}{
						"type": "text",
						"text": block.Text,
					})
				case "thinking":
					content = append(content, map[string]interface{}{
						"type":      "thinking",
						"thinking":  block.Thinking,
						"signature": block.Signature,
					})
				case "image":
					if block.ImageSource != nil {
						content = append(content, map[string]interface{}{
							"type": "image",
							"source": map[string]interface{}{
								"type":       "base64",
								"media_type": block.ImageSource.MimeType,
								"data":       block.ImageSource.Data,
							},
						})
					}
				case "tool_use":
					if block.ToolUse != nil {
						content = append(content, map[string]interface{}{
							"type":  "tool_use",
							"id":    block.ToolUse.ID,
							"name":  block.ToolUse.Function.Name,
							"input": parseToolInput(block.ToolUse.Function.Arguments),
						})
					}
				case "tool_result":
					if block.ToolResult != nil {
						m := map[string]interface{}{
							"type":        "tool_result",
							"tool_use_id": block.ToolResult.ToolUseID,
							"content":     block.ToolResult.Content,
						}
						if block.ToolResult.IsError {
							m["is_error"] = true
						}
						content = append(content, m)
					}
				case "redacted_thinking":
					content = append(content, map[string]interface{}{
						"type": "redacted_thinking",
					})
				case "signature":
					content = append(content, map[string]interface{}{
						"type":      "thinking",
						"thinking":  "",
						"signature": block.Signature,
					})
				}
			}
		} else {
			// Legacy fallback — handle ToolCalls, ToolResult, Thinking, Content
			if msg.Content != "" {
				content = append(content, map[string]interface{}{
					"type": "text",
					"text": msg.Content,
				})
			}
			if msg.Thinking != "" {
				content = append(content, map[string]interface{}{
					"type":      "thinking",
					"thinking":  msg.Thinking,
					"signature": "",
				})
			}
			for _, tc := range msg.ToolCalls {
				content = append(content, map[string]interface{}{
					"type":  "tool_use",
					"id":    tc.ID,
					"name":  tc.Function.Name,
					"input": parseToolInput(tc.Function.Arguments),
				})
			}
			if msg.ToolResult != nil {
				m := map[string]interface{}{
					"type":        "tool_result",
					"tool_use_id": msg.ToolResult.ToolUseID,
					"content":     msg.ToolResult.Content,
				}
				if msg.ToolResult.IsError {
					m["is_error"] = true
				}
				content = append(content, m)
			}
			if len(content) == 0 {
				content = append(content, map[string]interface{}{
					"type": "text",
					"text": "",
				})
			}
		}

		if len(content) > 0 {
			messages = append(messages, map[string]interface{}{
				"role":    msg.Role,
				"content": content,
			})
		}
	}

	// Add cache_control to last two user messages
	anthropicAddCacheControl(messages)

	model := req.Provider.Model
	if model == "" {
		model = "claude-sonnet-4-20250514"
	}

	maxTokens := req.MaxTokens
	if maxTokens == 0 {
		maxTokens = 8192
	}

	result := map[string]interface{}{
		"model":      model,
		"messages":   messages,
		"max_tokens": maxTokens,
	}

	// System prompt with cache breakpoint
	if req.System != "" {
		result["system"] = []map[string]interface{}{
			{
				"type":          "text",
				"text":          req.System,
				"cache_control": map[string]string{"type": "ephemeral"},
			},
		}
	}

	// Temperature — must be omitted when thinking is enabled
	reasoningOn := req.Thinking != nil && req.Thinking.BudgetTokens > 0
	if !reasoningOn && req.Temperature > 0 {
		result["temperature"] = req.Temperature
	}

	// TopP
	if req.TopP > 0 {
		result["top_p"] = req.TopP
	}

	// Stop sequences
	if len(req.Stop) > 0 {
		result["stop_sequences"] = req.Stop
	}

	// Thinking config
	if reasoningOn {
		result["thinking"] = map[string]interface{}{
			"type":          "enabled",
			"budget_tokens": req.Thinking.BudgetTokens,
		}
	}

	// Tools
	if len(req.Tools) > 0 {
		var tools []map[string]interface{}
		for _, toolJSON := range req.Tools {
			var tool struct {
				Name        string          `json:"name"`
				Description string          `json:"description"`
				InputSchema json.RawMessage `json:"input_schema"`
			}
			if err := json.Unmarshal(toolJSON, &tool); err != nil {
				continue
			}
			var schema interface{}
			if len(tool.InputSchema) > 0 {
				json.Unmarshal(tool.InputSchema, &schema)
			}
			if schema == nil {
				schema = map[string]interface{}{"type": "object"}
			}
			t := map[string]interface{}{
				"name":         tool.Name,
				"input_schema": schema,
			}
			if tool.Description != "" {
				t["description"] = tool.Description
			}
			tools = append(tools, t)
		}
		if len(tools) > 0 {
			result["tools"] = tools
			if !reasoningOn {
				result["tool_choice"] = map[string]string{"type": "auto"}
			}
		}
	}

	return result
}

// anthropicAddCacheControl adds cache_control: {type: "ephemeral"} to the last
// content block of the last two user messages, skipping thinking/redacted_thinking blocks.
func anthropicAddCacheControl(messages []map[string]interface{}) {
	userIndices := []int{}
	for i, msg := range messages {
		if msg["role"] == "user" {
			userIndices = append(userIndices, i)
		}
	}
	if len(userIndices) >= 1 {
		anthropicAddCacheControlToLastBlock(messages[userIndices[len(userIndices)-1]])
	}
	if len(userIndices) >= 2 {
		anthropicAddCacheControlToLastBlock(messages[userIndices[len(userIndices)-2]])
	}
}

func anthropicAddCacheControlToLastBlock(msg map[string]interface{}) {
	content, ok := msg["content"].([]map[string]interface{})
	if !ok || len(content) == 0 {
		return
	}
	lastIdx := -1
	for i := len(content) - 1; i >= 0; i-- {
		t, _ := content[i]["type"].(string)
		if t != "thinking" && t != "redacted_thinking" {
			lastIdx = i
			break
		}
	}
	if lastIdx < 0 {
		return
	}
	content[lastIdx]["cache_control"] = map[string]string{"type": "ephemeral"}
}

func (h *AnthropicHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.resolveConfig(req)
	payload := h.buildRequestBody(req)

	reqBody, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", baseURL+"/v1/messages", bytes.NewBuffer(reqBody))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("x-api-key", apiKey)
	}
	httpReq.Header.Set("anthropic-version", "2023-06-01")

	resp, err := SharedHTTPClient.Do(httpReq)
	if err != nil {
		return nil, wrapTransientError(fmt.Errorf("request failed: %w", err))
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, fmt.Errorf("failed to read response: %w", err)
	}

	if resp.StatusCode != http.StatusOK {
		return nil, newAPIErrorFromResp(resp, string(body))
	}

	return anthropicConvertResponse(body)
}

func anthropicConvertResponse(body []byte) (*SendResult, error) {
	var resp struct {
		Model      string `json:"model"`
		StopReason string `json:"stop_reason"`
		Content    []struct {
			Type      string          `json:"type"`
			Text      string          `json:"text"`
			Thinking  string          `json:"thinking"`
			Signature string          `json:"signature"`
			ID        string          `json:"id"`
			Name      string          `json:"name"`
			Input     json.RawMessage `json:"input"`
			ToolUseID string          `json:"tool_use_id"`
			Content   string          `json:"content"`
			IsError   bool            `json:"is_error"`
		} `json:"content"`
		Usage struct {
			InputTokens              int `json:"input_tokens"`
			OutputTokens             int `json:"output_tokens"`
			CacheCreationInputTokens int `json:"cache_creation_input_tokens"`
			CacheReadInputTokens     int `json:"cache_read_input_tokens"`
		} `json:"usage"`
	}
	if err := json.Unmarshal(body, &resp); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}

	var contentBlocks []ContentBlock
	for _, block := range resp.Content {
		switch block.Type {
		case "text":
			contentBlocks = append(contentBlocks, ContentBlock{
				Type: "text",
				Text: block.Text,
			})
		case "thinking":
			contentBlocks = append(contentBlocks, ContentBlock{
				Type:      "thinking",
				Thinking:  block.Thinking,
				Signature: block.Signature,
			})
		case "tool_use":
			contentBlocks = append(contentBlocks, ContentBlock{
				Type: "tool_use",
				ToolUse: &ToolUseBlock{
					ID:   block.ID,
					Type: "tool_use",
					Function: struct {
						Name      string `json:"name"`
						Arguments string `json:"arguments"`
					}{
						Name:      block.Name,
						Arguments: string(block.Input),
					},
				},
			})
		case "tool_result":
			contentBlocks = append(contentBlocks, ContentBlock{
				Type: "tool_result",
				ToolResult: &ToolResultBlock{
					ToolUseID: block.ToolUseID,
					Content:   block.Content,
					IsError:   block.IsError,
				},
			})
		case "redacted_thinking":
			contentBlocks = append(contentBlocks, ContentBlock{
				Type:     "thinking",
				Thinking: "[REDACTED]",
			})
		}
	}

	usage := &Usage{
		InputTokens:              resp.Usage.InputTokens,
		OutputTokens:             resp.Usage.OutputTokens,
		CacheCreationInputTokens: resp.Usage.CacheCreationInputTokens,
		CacheReadInputTokens:     resp.Usage.CacheReadInputTokens,
	}

	return &SendResult{
		Content:    contentBlocks,
		Model:      resp.Model,
		Usage:      usage,
		StopReason: resp.StopReason,
	}, nil
}

func (h *AnthropicHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.resolveConfig(req)
	payload := h.buildRequestBody(req)
	payload["stream"] = true

	reqBody, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("failed to marshal request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", baseURL+"/v1/messages", bytes.NewBuffer(reqBody))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("x-api-key", apiKey)
	}
	httpReq.Header.Set("anthropic-version", "2023-06-01")
	httpReq.Header.Set("Accept", "text/event-stream")

	resp, err := SharedHTTPClient.Do(httpReq)
	if err != nil {
		return wrapTransientError(fmt.Errorf("request failed: %w", err))
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
		return newAPIErrorFromResp(resp, string(body))
	}

	return anthropicParseSSE(ctx, &contextReader{ctx: ctx, r: resp.Body}, callback)
}

// anthropicParseSSE reads an Anthropic SSE stream and emits StreamChunks.
// Anthropic SSE uses event:<type> + data:<json> pairs.
func anthropicParseSSE(ctx context.Context, body io.Reader, callback func(StreamChunk) error) error {
	scanner := bufio.NewScanner(body)
	scanner.Buffer(make([]byte, 0, 32*1024), 256*1024)

	var eventType string

	for scanner.Scan() {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		line := scanner.Text()

		if strings.HasPrefix(line, "event: ") {
			eventType = strings.TrimPrefix(line, "event: ")
			continue
		}
		if !strings.HasPrefix(line, "data: ") {
			continue
		}
		data := []byte(strings.TrimPrefix(line, "data: "))

		switch eventType {
		case "message_start":
			var ev struct {
			 Message struct {
					Model string `json:"model"`
				} `json:"message"`
			}
			if err := json.Unmarshal(data, &ev); err == nil {
				if err := callback(StreamChunk{Type: "start", Content: ev.Message.Model}); err != nil {
					return err
				}
			}

		case "content_block_start":
			var ev struct {
				Index        int                    `json:"index"`
				ContentBlock map[string]interface{} `json:"content_block"`
			}
			if err := json.Unmarshal(data, &ev); err == nil {
				blockType, _ := ev.ContentBlock["type"].(string)
				chunk := StreamChunk{
					Type:    "content",
					Index:   ev.Index,
					Content: blockType,
				}
				if blockType == "tool_use" {
					chunk.ToolCallID, _ = ev.ContentBlock["id"].(string)
					chunk.ToolCallName, _ = ev.ContentBlock["name"].(string)
				}
				if err := callback(chunk); err != nil {
					return err
				}
			}

		case "content_block_delta":
			var ev struct {
				Index int                    `json:"index"`
				Delta map[string]interface{} `json:"delta"`
			}
			if err := json.Unmarshal(data, &ev); err == nil {
				deltaType, _ := ev.Delta["type"].(string)
				chunk := StreamChunk{Type: "delta", Index: ev.Index}
				switch deltaType {
				case "text_delta":
					chunk.TextDelta, _ = ev.Delta["text"].(string)
				case "thinking_delta":
					chunk.Thinking, _ = ev.Delta["thinking"].(string)
				case "input_json_delta":
					chunk.JSONDelta, _ = ev.Delta["partial_json"].(string)
				case "signature_delta":
					chunk.Thinking, _ = ev.Delta["signature"].(string)
				}
				if err := callback(chunk); err != nil {
					return err
				}
			}

		case "message_delta":
			var ev struct {
				Delta struct {
					StopReason string `json:"stop_reason"`
				} `json:"delta"`
				Usage struct {
					OutputTokens int `json:"output_tokens"`
				} `json:"usage"`
			}
			if err := json.Unmarshal(data, &ev); err == nil {
				if err := callback(StreamChunk{
					Type:         "stop",
					FinishReason: ev.Delta.StopReason,
					Usage:        &Usage{OutputTokens: ev.Usage.OutputTokens},
				}); err != nil {
					return err
				}
			}

		case "message_stop":
			if err := callback(StreamChunk{Type: "complete"}); err != nil {
				return err
			}
		}
	}
	return scanner.Err()
}

// ListModels fetches available models from the Anthropic API.
func (h *AnthropicHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := "https://api.anthropic.com"
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	apiKey := cfg.APIKey

	client := SharedHTTPClient
	req, err := http.NewRequestWithContext(ctx, "GET", base+"/v1/models?limit=1000", nil)
	if err != nil {
		return nil, err
	}
	if apiKey != "" {
		req.Header.Set("x-api-key", apiKey)
	}
	req.Header.Set("anthropic-version", "2023-06-01")

	resp, err := client.Do(req)
	if err != nil {
		return nil, wrapTransientError(err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		errBody, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
		return nil, fmt.Errorf("Anthropic /models returned status %d: %s", resp.StatusCode, string(errBody))
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, err
	}

	var result struct {
		Data []struct {
			ID   string `json:"id"`
			Name string `json:"display_name,omitempty"`
		} `json:"data"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, err
	}

	entries := make([]ModelEntry, 0, len(result.Data))
	for _, m := range result.Data {
		entries = append(entries, ModelEntry{
			ID:   m.ID,
			Name: m.Name,
		})
	}
	return entries, nil
}

func (h *AnthropicHandler) Capabilities() *ProviderInfo {
	return &ProviderInfo{
		ID:               "anthropic",
		DefaultModel:     "claude-sonnet-4-20250514",
		MaxTokensDefault: 8192,
		Features: ProviderFeatures{
			SupportsThinking:        true,
			SupportsReasoningEffort: false,
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
				Max:         fPtr(1),
				Step:        fPtr(0.01),
				Default:     1.0,
				Group:       "sampling",
				Description: "Controls randomness (0 = deterministic, 1 = creative). Ignored in thinking mode.",
				ValidRange:  "0 – 1",
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
				Max:         fPtr(500),
				Step:        fPtr(1),
				Group:       "sampling",
				Description: "Only sample from the top K options for each subsequent token.",
				ValidRange:  "0 – 500",
			},
			{
				Key:         "stop_sequences",
				Label:       "Stop Sequences",
				Type:        SettingText,
				Group:       "sampling",
				Description: "Custom stop sequences (comma-separated).",
			},
			{
				Key:         "thinking_budget_tokens",
				Label:       "Thinking Budget Tokens",
				Type:        SettingNumber,
				Min:         fPtr(1024),
				Default:     8192,
				Group:       "reasoning",
				Description: "Token budget for extended thinking. Only applies in thinking mode.",
			},
		},
	}
}

func (h *AnthropicHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.Capabilities(), settings, thinking,
		InactiveInThinking("top_k"),
		CrossParamRule(func(key string, val interface{}, allSettings map[string]interface{}) *SettingValidation {
			if key == "thinking_budget_tokens" {
				isThinking := thinking != nil && thinking.Type == "enabled"
				if !isThinking {
					return &SettingValidation{
						Status:  StatusInactive,
						Message: "Only applies in thinking mode",
					}
				}
				if n := toFloat(val); n > 0 && n < 1024 {
					return &SettingValidation{
						Error: "Anthropic requires budget_tokens >= 1024",
						Value: 1024,
					}
				}
			}
			return nil
		}),
	)
}

var _ CapableHandler = (*AnthropicHandler)(nil)
var _ SettingsValidator = (*AnthropicHandler)(nil)

var _ ModelLister = (*AnthropicHandler)(nil)
