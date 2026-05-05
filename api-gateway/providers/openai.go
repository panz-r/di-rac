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

// OpenAIHandler handles OpenAI-compatible API requests
type OpenAIHandler struct {
	httpClient *http.Client
	baseURL    string
	apiKey     string
}

func NewOpenAIHandler() *OpenAIHandler {
	return &OpenAIHandler{
		httpClient: &http.Client{},
		baseURL:    "https://api.openai.com/v1",
	}
}

func NewOpenAIHandlerWithKey(apiKey string) *OpenAIHandler {
	return &OpenAIHandler{
		httpClient: &http.Client{},
		baseURL:    "https://api.openai.com/v1",
		apiKey:     apiKey,
	}
}

func (h *OpenAIHandler) getConfig(req *Request) (baseURL, apiKey string) {
	baseURL = h.baseURL
	apiKey = h.apiKey
	if req.Provider.BaseURL != "" {
		baseURL = req.Provider.BaseURL
	}
	if req.Provider.APIKey != "" {
		apiKey = req.Provider.APIKey
	}
	return
}

func (h *OpenAIHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.getConfig(req)
	openaiReq := h.buildRequest(req, false)

	reqBody, err := json.Marshal(openaiReq)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", baseURL+"/chat/completions", bytes.NewBuffer(reqBody))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	h.setHeaders(httpReq, apiKey)

	resp, err := h.httpClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("failed to read response: %w", err)
	}

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("API error (status %d): %s", resp.StatusCode, string(body))
	}

	var openaiResp map[string]interface{}
	if err := json.Unmarshal(body, &openaiResp); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}

	return h.convertResponse(openaiResp), nil
}

func (h *OpenAIHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.getConfig(req)
	openaiReq := h.buildRequest(req, true)

	reqBody, err := json.Marshal(openaiReq)
	if err != nil {
		return fmt.Errorf("failed to marshal request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", baseURL+"/chat/completions", bytes.NewBuffer(reqBody))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	h.setHeaders(httpReq, apiKey)
	httpReq.Header.Set("Accept", "text/event-stream")
	httpReq.Header.Set("Cache-Control", "no-cache")

	resp, err := h.httpClient.Do(httpReq)
	if err != nil {
		return fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("API error (status %d): %s", resp.StatusCode, string(body))
	}

	return h.parseSSEStream(resp.Body, callback)
}

func (h *OpenAIHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
}

func (h *OpenAIHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	messages := h.convertMessages(req)
	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = "gpt-4o"
	}

	result := map[string]interface{}{
		"model":    model,
		"messages": messages,
	}

	if stream {
		result["stream"] = true
		result["stream_options"] = map[string]interface{}{"include_usage": true}
	}

	if req.MaxTokens > 0 {
		result["max_tokens"] = req.MaxTokens
	}
	if req.Temperature > 0 {
		result["temperature"] = req.Temperature
	}
	if req.TopP > 0 {
		result["top_p"] = req.TopP
	}
	if len(req.Stop) > 0 {
		result["stop"] = req.Stop
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
			var inputSchema interface{}
			if len(tool.InputSchema) > 0 {
				json.Unmarshal(tool.InputSchema, &inputSchema)
			}
			if inputSchema == nil {
				inputSchema = map[string]interface{}{"type": "object"}
			}
			toolMap := map[string]interface{}{
				"type": "function",
				"function": map[string]interface{}{
					"name":        tool.Name,
					"description": tool.Description,
					"parameters":  inputSchema,
				},
			}
			tools = append(tools, toolMap)
		}
		if len(tools) > 0 {
			result["tools"] = tools
			result["tool_choice"] = "auto"
		}
	}

	return result
}

// convertMessages converts DiracStorageMessage-format messages to OpenAI chat completion format.
// Replicates the logic from convertToOpenAiMessages in openai-format.ts.
func (h *OpenAIHandler) convertMessages(req *Request) []map[string]interface{} {
	var messages []map[string]interface{}

	// System prompt as first message
	if req.System != "" {
		messages = append(messages, map[string]interface{}{
			"role":    "system",
			"content": req.System,
		})
	}

	for _, msg := range req.Messages {
		if len(msg.ContentBlocks) > 0 {
			messages = h.convertContentBlockMessage(messages, msg)
			continue
		}
		// Legacy fallback
		m := map[string]interface{}{"role": msg.Role}
		if msg.Content != "" {
			m["content"] = msg.Content
		}
		if len(msg.ToolCalls) > 0 {
			var toolCalls []map[string]interface{}
			for _, tc := range msg.ToolCalls {
				toolCalls = append(toolCalls, map[string]interface{}{
					"id":   tc.ID,
					"type": "function",
					"function": map[string]interface{}{
						"name":      tc.Function.Name,
						"arguments": tc.Function.Arguments,
					},
				})
			}
			m["tool_calls"] = toolCalls
			if msg.Content == "" {
				m["content"] = nil
			}
		}
		if msg.ToolResult != nil {
			m["role"] = "tool"
			m["tool_call_id"] = msg.ToolResult.ToolUseID
			m["content"] = msg.ToolResult.Content
		}
		messages = append(messages, m)
	}

	return messages
}

// convertContentBlockMessage handles messages with content blocks, converting
// tool_use → tool_calls, tool_result → role: "tool", images → image_url format.
func (h *OpenAIHandler) convertContentBlockMessage(messages []map[string]interface{}, msg Message) []map[string]interface{} {
	if msg.Role == "user" {
		return h.convertUserContentBlocks(messages, msg)
	}
	// Assistant
	return h.convertAssistantContentBlocks(messages, msg)
}

func (h *OpenAIHandler) convertUserContentBlocks(messages []map[string]interface{}, msg Message) []map[string]interface{} {
	var textParts []map[string]interface{}
	var toolResultParts []ContentBlock
	var imageParts []map[string]interface{}

	for _, block := range msg.ContentBlocks {
		switch block.Type {
		case "text":
			textParts = append(textParts, map[string]interface{}{
				"type": "text",
				"text": block.Text,
			})
		case "image":
			if block.ImageSource != nil {
				var url string
				if block.ImageSource.Data != "" {
					url = "data:" + block.ImageSource.MimeType + ";base64," + block.ImageSource.Data
				} else if block.ImageSource.URL != "" {
					url = block.ImageSource.URL
				}
				if url != "" {
					imageParts = append(imageParts, map[string]interface{}{
						"type": "image_url",
						"image_url": map[string]interface{}{
							"url": url,
						},
					})
				}
			}
		case "tool_result":
			toolResultParts = append(toolResultParts, block)
		}
	}

	// Emit tool results as role: "tool" messages FIRST
	for _, block := range toolResultParts {
		if block.ToolResult == nil {
			continue
		}
		content := block.ToolResult.Content
		if content == "" {
			content = ""
		}
		messages = append(messages, map[string]interface{}{
			"role":         "tool",
			"tool_call_id": block.ToolResult.ToolUseID,
			"content":      content,
		})
	}

	// Then emit user message with text + images
	content := append(textParts, imageParts...)
	if len(content) > 0 {
		messages = append(messages, map[string]interface{}{
			"role":    "user",
			"content": content,
		})
	}

	return messages
}

func (h *OpenAIHandler) convertAssistantContentBlocks(messages []map[string]interface{}, msg Message) []map[string]interface{} {
	var textParts []string
	var toolCalls []map[string]interface{}
	var reasoningContent string

	for _, block := range msg.ContentBlocks {
		switch block.Type {
		case "text":
			textParts = append(textParts, block.Text)
		case "thinking":
			reasoningContent += block.Thinking
		case "tool_use":
			if block.ToolUse != nil {
				args := block.ToolUse.Function.Arguments
				if args == "" {
					args = "{}"
				}
				toolCalls = append(toolCalls, map[string]interface{}{
					"id":   block.ToolUse.ID,
					"type": "function",
					"function": map[string]interface{}{
						"name":      block.ToolUse.Function.Name,
						"arguments": args,
					},
				})
			}
		}
	}

	m := map[string]interface{}{
		"role": "assistant",
	}
	if len(textParts) > 0 {
		m["content"] = strings.Join(textParts, "\n")
	} else if len(toolCalls) > 0 {
		m["content"] = nil
	} else {
		m["content"] = ""
	}
	if len(toolCalls) > 0 {
		m["tool_calls"] = toolCalls
	}
	if reasoningContent != "" {
		m["reasoning_content"] = reasoningContent
	}
	messages = append(messages, m)
	return messages
}

// parseSSEStream reads an SSE stream and emits StreamChunks.
// Handles text deltas, tool_call deltas, reasoning_content, finish_reason, and usage.
func (h *OpenAIHandler) parseSSEStream(body io.Reader, callback func(StreamChunk) error) error {
	// Tool call accumulator: index → {id, name}
	type toolCallState struct {
		id   string
		name string
	}
	toolCalls := make(map[int]*toolCallState)

	scanner := bufio.NewScanner(body)
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	for scanner.Scan() {
		line := scanner.Text()
		if !strings.HasPrefix(line, "data: ") {
			continue
		}
		data := strings.TrimPrefix(line, "data: ")
		if data == "[DONE]" {
			callback(StreamChunk{Type: "complete"})
			return nil
		}

		var chunk struct {
			Choices []struct {
				Delta struct {
					Content          string `json:"content"`
					ReasoningContent string `json:"reasoning_content"`
					ToolCalls        []struct {
						Index    int    `json:"index"`
						ID       string `json:"id"`
						Type     string `json:"type"`
						Function struct {
							Name      string `json:"name"`
							Arguments string `json:"arguments"`
						} `json:"function"`
					} `json:"tool_calls"`
				} `json:"delta"`
				FinishReason *string `json:"finish_reason"`
			} `json:"choices"`
			Usage struct {
				PromptTokens     int `json:"prompt_tokens"`
				CompletionTokens int `json:"completion_tokens"`
				PromptTokensDetails struct {
					CachedTokens int `json:"cached_tokens"`
				} `json:"prompt_tokens_details"`
			} `json:"usage"`
		}

		if err := json.Unmarshal([]byte(data), &chunk); err != nil {
			continue
		}

		if len(chunk.Choices) == 0 {
			// Usage-only chunk (no choices)
			if chunk.Usage.PromptTokens > 0 || chunk.Usage.CompletionTokens > 0 {
				usage := &Usage{
					InputTokens:      chunk.Usage.PromptTokens,
					OutputTokens:     chunk.Usage.CompletionTokens,
					CacheReadInputTokens:  chunk.Usage.PromptTokensDetails.CachedTokens,
				}
				callback(StreamChunk{Type: "stop", Usage: usage})
			}
			continue
		}

		choice := chunk.Choices[0]
		delta := choice.Delta

		// Text content
		if delta.Content != "" {
			callback(StreamChunk{Type: "delta", TextDelta: delta.Content})
		}

		// Reasoning content
		if delta.ReasoningContent != "" {
			callback(StreamChunk{Type: "delta", Thinking: delta.ReasoningContent})
		}

		// Tool calls — accumulate and emit when all fields present
		for _, tc := range delta.ToolCalls {
			idx := tc.Index
			state, ok := toolCalls[idx]
			if !ok {
				state = &toolCallState{}
				toolCalls[idx] = state
			}
			if tc.ID != "" {
				state.id = tc.ID
			}
			if tc.Function.Name != "" {
				state.name = tc.Function.Name
			}
			// Only emit when we have id, name, and arguments
			if state.id != "" && state.name != "" && tc.Function.Arguments != "" {
				callback(StreamChunk{
					Type:     "delta",
					Index:    idx,
					JSONDelta: tc.Function.Arguments,
					ToolCallID:   state.id,
					ToolCallName: state.name,
				})
			}
		}

		// Finish reason
		if choice.FinishReason != nil {
			finishReason := *choice.FinishReason
			usage := &Usage{}
			if chunk.Usage.PromptTokens > 0 || chunk.Usage.CompletionTokens > 0 {
				usage.InputTokens = chunk.Usage.PromptTokens
				usage.OutputTokens = chunk.Usage.CompletionTokens
				usage.CacheReadInputTokens = chunk.Usage.PromptTokensDetails.CachedTokens
			}
			callback(StreamChunk{Type: "stop", FinishReason: finishReason, Usage: usage})
		}
	}

	return nil
}

func (h *OpenAIHandler) convertResponse(resp map[string]interface{}) *SendResult {
	var content []ContentBlock

	if choices, ok := resp["choices"].([]interface{}); ok && len(choices) > 0 {
		if choice, ok := choices[0].(map[string]interface{}); ok {
			if msg, ok := choice["message"].(map[string]interface{}); ok {
				// Text content
				if text, ok := msg["content"].(string); ok && text != "" {
					content = append(content, ContentBlock{Type: "text", Text: text})
				}
				// Tool calls
				if toolCalls, ok := msg["tool_calls"].([]interface{}); ok {
					for _, tc := range toolCalls {
						if tcMap, ok := tc.(map[string]interface{}); ok {
							id, _ := tcMap["id"].(string)
							if fn, ok := tcMap["function"].(map[string]interface{}); ok {
								name, _ := fn["name"].(string)
								args, _ := fn["arguments"].(string)
								content = append(content, ContentBlock{
									Type: "tool_use",
									ToolUse: &ToolUseBlock{
										ID:   id,
										Type: "tool_use",
										Function: struct {
											Name      string `json:"name"`
											Arguments string `json:"arguments"`
										}{Name: name, Arguments: args},
									},
								})
							}
						}
					}
				}
			}
		}
	}

	usage := &Usage{}
	if usageMap, ok := resp["usage"].(map[string]interface{}); ok {
		if tokens, ok := usageMap["prompt_tokens"].(float64); ok {
			usage.InputTokens = int(tokens)
		}
		if tokens, ok := usageMap["completion_tokens"].(float64); ok {
			usage.OutputTokens = int(tokens)
		}
		if tokens, ok := usageMap["total_tokens"].(float64); ok {
			usage.TotalTokens = int(tokens)
		}
	}

	stopReason := ""
	if choices, ok := resp["choices"].([]interface{}); ok && len(choices) > 0 {
		if choice, ok := choices[0].(map[string]interface{}); ok {
			if sr, ok := choice["finish_reason"].(string); ok {
				stopReason = sr
			}
		}
	}

	model := ""
	if modelStr, ok := resp["model"].(string); ok {
		model = modelStr
	}

	return &SendResult{
		Content:    content,
		Model:      model,
		Usage:      usage,
		StopReason: stopReason,
	}
}

func (h *OpenAIHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.baseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", h.apiKey)
}

var _ ModelLister = (*OpenAIHandler)(nil)
