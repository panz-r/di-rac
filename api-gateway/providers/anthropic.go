package providers

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"

	"github.com/anthropics/anthropic-sdk-go"
	"github.com/anthropics/anthropic-sdk-go/option"
)

// AnthropicHandler handles Anthropic API requests
type AnthropicHandler struct {
	client       anthropic.Client
	clientCache  sync.Map
}

func NewAnthropicHandler() *AnthropicHandler {
	return &AnthropicHandler{
		client: anthropic.NewClient(),
	}
}

func NewAnthropicHandlerWithKey(apiKey string) *AnthropicHandler {
	return &AnthropicHandler{
		client: anthropic.NewClient(option.WithAPIKey(apiKey)),
	}
}

func (h *AnthropicHandler) getClient(req *Request) anthropic.Client {
	if req.Provider.APIKey == "" && req.Provider.BaseURL == "" {
		return h.client
	}
	cacheKey := req.Provider.APIKey + "|" + req.Provider.BaseURL
	if cached, ok := h.clientCache.Load(cacheKey); ok {
		return cached.(anthropic.Client)
	}
	var opts []option.RequestOption
	if req.Provider.APIKey != "" {
		opts = append(opts, option.WithAPIKey(req.Provider.APIKey))
	}
	if req.Provider.BaseURL != "" {
		opts = append(opts, option.WithBaseURL(req.Provider.BaseURL))
	}
	client := anthropic.NewClient(opts...)
	h.clientCache.Store(cacheKey, client)
	return client
}

func (h *AnthropicHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	anthropicReq, err := h.buildRequest(req)
	if err != nil {
		return nil, err
	}
	client := h.getClient(req)
	resp, err := client.Messages.New(ctx, anthropicReq)
	if err != nil {
		return nil, fmt.Errorf("anthropic request failed: %w", err)
	}
	return h.convertResponse(resp), nil
}

func (h *AnthropicHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	anthropicReq, err := h.buildRequest(req)
	if err != nil {
		return err
	}
	client := h.getClient(req)
	stream := client.Messages.NewStreaming(ctx, anthropicReq)
	for stream.Next() {
		event := stream.Current()
		chunk := h.convertStreamEvent(event)
		if err := callback(chunk); err != nil {
			return err
		}
	}
	if err := stream.Err(); err != nil {
		return fmt.Errorf("anthropic stream failed: %w", err)
	}
	return nil
}

func (h *AnthropicHandler) buildRequest(req *Request) (anthropic.MessageNewParams, error) {
	var messages []anthropic.MessageParam

	for _, msg := range req.Messages {
		if len(msg.ContentBlocks) > 0 {
			var contentBlocks []anthropic.ContentBlockParamUnion
			for _, block := range msg.ContentBlocks {
				switch block.Type {
				case "text":
					contentBlocks = append(contentBlocks, anthropic.NewTextBlock(block.Text))
				case "thinking":
					contentBlocks = append(contentBlocks, anthropic.NewThinkingBlock(block.Signature, block.Thinking))
				case "image":
					if block.ImageSource != nil {
						contentBlocks = append(contentBlocks, anthropic.NewImageBlockBase64(
							block.ImageSource.MimeType,
							block.ImageSource.Data,
						))
					}
				case "tool_use":
					if block.ToolUse != nil {
						contentBlocks = append(contentBlocks, anthropic.NewToolUseBlock(
							block.ToolUse.ID,
							block.ToolUse.Function.Arguments,
							block.ToolUse.Function.Name,
						))
					}
				case "tool_result":
					if block.ToolResult != nil {
						contentBlocks = append(contentBlocks, anthropic.NewToolResultBlock(
							block.ToolResult.ToolUseID,
							block.ToolResult.Content,
							block.ToolResult.IsError,
						))
					}
				case "redacted_thinking":
					contentBlocks = append(contentBlocks, anthropic.NewRedactedThinkingBlock(""))
				case "signature":
					contentBlocks = append(contentBlocks, anthropic.NewThinkingBlock(block.Signature, ""))
				}
			}
			var msgParam anthropic.MessageParam
			if msg.Role == "user" {
				msgParam = anthropic.NewUserMessage(contentBlocks...)
			} else {
				msgParam = anthropic.NewAssistantMessage(contentBlocks...)
			}
			messages = append(messages, msgParam)
			continue
		}

		// Legacy fallback — handle ToolCalls, ToolResult, Thinking, Content
		var contentBlocks []anthropic.ContentBlockParamUnion
		if msg.Content != "" {
			contentBlocks = append(contentBlocks, anthropic.NewTextBlock(msg.Content))
		}
		if msg.Thinking != "" {
			contentBlocks = append(contentBlocks, anthropic.NewThinkingBlock("", msg.Thinking))
		}
		for _, tc := range msg.ToolCalls {
			contentBlocks = append(contentBlocks, anthropic.NewToolUseBlock(
				tc.ID,
				tc.Function.Arguments,
				tc.Function.Name,
			))
		}
		if msg.ToolResult != nil {
			contentBlocks = append(contentBlocks, anthropic.NewToolResultBlock(
				msg.ToolResult.ToolUseID,
				msg.ToolResult.Content,
				msg.ToolResult.IsError,
			))
		}
		if len(contentBlocks) == 0 {
			contentBlocks = append(contentBlocks, anthropic.NewTextBlock(""))
		}
		var msgParam anthropic.MessageParam
		if msg.Role == "user" {
			msgParam = anthropic.NewUserMessage(contentBlocks...)
		} else {
			msgParam = anthropic.NewAssistantMessage(contentBlocks...)
		}
		messages = append(messages, msgParam)
	}

	// Add cache_control to last two user messages
	addCacheControlToUserMessages(messages)

	model := req.Provider.Model
	if model == "" {
		model = "claude-sonnet-4-20250514"
	}

	maxTokens := int64(req.MaxTokens)
	if maxTokens == 0 {
		maxTokens = 8192
	}

	anthropicReq := anthropic.MessageNewParams{
		Model:     anthropic.Model(model),
		Messages:  messages,
		MaxTokens: maxTokens,
	}

	// System prompt with cache breakpoint
	if req.System != "" {
		anthropicReq.System = []anthropic.TextBlockParam{
			{
				Text: req.System,
				CacheControl: anthropic.CacheControlEphemeralParam{
					Type: "ephemeral",
				},
			},
		}
	}

	// Temperature — must be nil when thinking is enabled
	reasoningOn := req.Thinking != nil && req.Thinking.BudgetTokens > 0
	if !reasoningOn && req.Temperature > 0 {
		anthropicReq.Temperature = anthropic.Float(req.Temperature)
	}

	// TopP
	if req.TopP > 0 {
		anthropicReq.TopP = anthropic.Float(req.TopP)
	}

	// Stop sequences
	if len(req.Stop) > 0 {
		stopSlice := make([]string, len(req.Stop))
		copy(stopSlice, req.Stop)
		anthropicReq.StopSequences = stopSlice
	}

	// Thinking config
	if reasoningOn {
		anthropicReq.Thinking = anthropic.ThinkingConfigParamOfEnabled(int64(req.Thinking.BudgetTokens))
	}

	// Tools
	if len(req.Tools) > 0 {
		var tools []anthropic.ToolUnionParam
		for _, toolJSON := range req.Tools {
			var tool struct {
				Name        string          `json:"name"`
				Description string          `json:"description"`
				InputSchema json.RawMessage `json:"input_schema"`
			}
			if err := json.Unmarshal(toolJSON, &tool); err != nil {
				continue
			}
			inputSchema := anthropic.ToolInputSchemaParam{Type: "object"}
			if len(tool.InputSchema) > 0 {
				var schemaMap map[string]interface{}
				if json.Unmarshal(tool.InputSchema, &schemaMap) == nil {
					if props, ok := schemaMap["properties"]; ok {
						inputSchema.Properties = props
					}
					if extraFields, ok := schemaMap["required"]; ok {
						if inputSchema.ExtraFields == nil {
							inputSchema.ExtraFields = map[string]any{}
						}
						inputSchema.ExtraFields["required"] = extraFields
					}
				}
			}
			toolParam := anthropic.ToolParam{
				Name:        tool.Name,
				InputSchema: inputSchema,
			}
			if tool.Description != "" {
				toolParam.Description = anthropic.String(tool.Description)
			}
			tools = append(tools, anthropic.ToolUnionParam{OfTool: &toolParam})
		}
		if len(tools) > 0 {
			anthropicReq.Tools = tools
			if !reasoningOn {
				anthropicReq.ToolChoice = anthropic.ToolChoiceUnionParam{OfAny: &anthropic.ToolChoiceAnyParam{}}
			}
		}
	}

	return anthropicReq, nil
}

// addCacheControlToUserMessages adds cache_control: {type: "ephemeral"} to the
// last content block of the last two user messages, skipping thinking/redacted_thinking blocks.
func addCacheControlToUserMessages(messages []anthropic.MessageParam) {
	userIndices := []int{}
	for i, msg := range messages {
		if msg.Role == "user" {
			userIndices = append(userIndices, i)
		}
	}
	if len(userIndices) >= 1 {
		addCacheControlToMessage(&messages[userIndices[len(userIndices)-1]])
	}
	if len(userIndices) >= 2 {
		addCacheControlToMessage(&messages[userIndices[len(userIndices)-2]])
	}
}

func addCacheControlToMessage(msg *anthropic.MessageParam) {
	// The content is in the union field — we need to access the underlying content blocks.
	// Since anthropic.MessageParam uses a union type, we work with the JSON representation.
	// The cache_control is added to the last block that isn't thinking/redacted_thinking.
	//
	// Note: The Anthropic Go SDK's MessageParam uses Content field which is []ContentBlockParamUnion.
	// We can't easily mutate the union, so we reconstruct. However, since the SDK uses
	// value types with embedded interfaces, this is done via JSON round-trip.
	data, err := json.Marshal(msg)
	if err != nil {
		return
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return
	}
	contentRaw, ok := raw["content"]
	if !ok {
		return
	}
	var blocks []json.RawMessage
	if err := json.Unmarshal(contentRaw, &blocks); err != nil {
		return
	}
	if len(blocks) == 0 {
		return
	}
	// Find last non-thinking block
	lastIdx := -1
	for i := len(blocks) - 1; i >= 0; i-- {
		var typeCheck struct {
			Type string `json:"type"`
		}
		if json.Unmarshal(blocks[i], &typeCheck) == nil {
			if typeCheck.Type != "thinking" && typeCheck.Type != "redacted_thinking" {
				lastIdx = i
				break
			}
		}
	}
	if lastIdx < 0 {
		return
	}
	// Add cache_control to that block
	var block map[string]json.RawMessage
	if json.Unmarshal(blocks[lastIdx], &block) != nil {
		return
	}
	block["cache_control"] = json.RawMessage(`{"type":"ephemeral"}`)
	updated, err := json.Marshal(block)
	if err != nil {
		return
	}
	blocks[lastIdx] = updated
	updatedContent, err := json.Marshal(blocks)
	if err != nil {
		return
	}
	raw["content"] = updatedContent
	updatedMsg, err := json.Marshal(raw)
	if err != nil {
		return
	}
	json.Unmarshal(updatedMsg, msg)
}

func (h *AnthropicHandler) convertResponse(resp *anthropic.Message) *SendResult {
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
			var toolUseID string
			var content string
			var isError bool
			if jsonData, err := json.Marshal(block); err == nil {
				var tmp struct {
					ToolUseID string `json:"tool_use_id"`
					Content   string `json:"content"`
					IsError   bool   `json:"is_error"`
				}
				if json.Unmarshal(jsonData, &tmp) == nil {
					toolUseID = tmp.ToolUseID
					content = tmp.Content
					isError = tmp.IsError
				}
			}
			contentBlocks = append(contentBlocks, ContentBlock{
				Type: "tool_result",
				ToolResult: &ToolResultBlock{
					ToolUseID: toolUseID,
					Content:   content,
					IsError:   isError,
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
		InputTokens:              int(resp.Usage.InputTokens),
		OutputTokens:             int(resp.Usage.OutputTokens),
		CacheCreationInputTokens: int(resp.Usage.CacheCreationInputTokens),
		CacheReadInputTokens:     int(resp.Usage.CacheReadInputTokens),
	}

	return &SendResult{
		Content:    contentBlocks,
		Model:      string(resp.Model),
		Usage:      usage,
		StopReason: string(resp.StopReason),
	}
}

func (h *AnthropicHandler) convertStreamEvent(event anthropic.MessageStreamEventUnion) StreamChunk {
	switch event.Type {
	case "message_start":
		return StreamChunk{Type: "start", Content: string(event.Message.Model)}
	case "content_block_start":
		cb := event.AsContentBlockStart()
		contentType := "unknown"
		var toolCallID, toolCallName string
		switch cb.Type {
		case "text":
			contentType = "text"
		case "thinking":
			contentType = "thinking"
		case "tool_use":
			contentType = "tool_use"
			tu := cb.ContentBlock.AsToolUse()
			toolCallID = tu.ID
			toolCallName = tu.Name
		case "signature":
			contentType = "signature"
		}
		return StreamChunk{
			Type:         "content",
			Index:        int(event.Index),
			Content:      contentType,
			ToolCallID:   toolCallID,
			ToolCallName: toolCallName,
		}
	case "content_block_delta":
		delta := event.AsContentBlockDelta()
		switch delta.Delta.Type {
		case "text_delta":
			return StreamChunk{Type: "delta", Index: int(event.Index), TextDelta: delta.Delta.Text}
		case "thinking_delta":
			return StreamChunk{Type: "delta", Index: int(event.Index), Thinking: delta.Delta.Thinking}
		case "input_json_delta":
			return StreamChunk{Type: "delta", Index: int(event.Index), JSONDelta: delta.Delta.PartialJSON}
		case "signature_delta":
			return StreamChunk{Type: "delta", Index: int(event.Index), Thinking: delta.Delta.Signature}
		}
		return StreamChunk{Type: "delta", Index: int(event.Index)}
	case "message_delta":
		delta := event.AsMessageDelta()
		usage := &Usage{
			OutputTokens: int(delta.Usage.OutputTokens),
		}
		return StreamChunk{Type: "stop", FinishReason: string(delta.Delta.StopReason), Usage: usage}
	case "message_stop":
		return StreamChunk{Type: "complete"}
	default:
		return StreamChunk{Type: "unknown"}
	}
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
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		errBody, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("Anthropic /models returned status %d: %s", resp.StatusCode, string(errBody))
	}

	body, err := io.ReadAll(resp.Body)
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
		ID:           "anthropic",
		DefaultModel: "claude-sonnet-4-20250514",
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
				Key:         "max_tokens",
				Label:       "Max Tokens",
				Type:        SettingNumber,
				Min:         fPtr(1),
				Default:     8192,
				Group:       "sampling",
				Description: "Maximum number of tokens to generate.",
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
			}
			return nil
		}),
	)
}

var _ CapableHandler = (*AnthropicHandler)(nil)
var _ SettingsValidator = (*AnthropicHandler)(nil)

var _ ModelLister = (*AnthropicHandler)(nil)
