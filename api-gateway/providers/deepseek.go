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

// DeepSeekHandler handles DeepSeek API requests.
// DeepSeek uses OpenAI-compatible chat completions with:
//   - reasoning_content field for thinking
//   - R1 format (merged consecutive same-role messages)
//   - addReasoningContent (round-trip thinking blocks as reasoning_content)
//   - strict: true on tool functions
//   - max_completion_tokens instead of max_tokens
type DeepSeekHandler struct {
	httpClient *http.Client
	baseURL    string
	apiKey     string
}

func NewDeepSeekHandler() *DeepSeekHandler {
	return &DeepSeekHandler{
		httpClient: &http.Client{},
		baseURL:    "https://api.deepseek.com/v1",
	}
}

func NewDeepSeekHandlerWithKey(apiKey string) *DeepSeekHandler {
	return &DeepSeekHandler{
		httpClient: &http.Client{},
		baseURL:    "https://api.deepseek.com/v1",
		apiKey:     apiKey,
	}
}

func (h *DeepSeekHandler) getConfig(req *Request) (baseURL, apiKey string) {
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

func (h *DeepSeekHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.getConfig(req)
	deepseekReq := h.buildRequest(req, false)

	reqBody, err := json.Marshal(deepseekReq)
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

func (h *DeepSeekHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.getConfig(req)
	deepseekReq := h.buildRequest(req, true)

	reqBody, err := json.Marshal(deepseekReq)
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

func (h *DeepSeekHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
}

func (h *DeepSeekHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	// Use the OpenAI handler's message conversion
	openai := &OpenAIHandler{}
	messages := openai.convertMessages(req)

	// Apply DeepSeek-specific message transformations
	messages = h.addReasoningContent(messages, req)
	messages = h.coerceAssistantMessages(messages)

	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = "deepseek-chat"
	}

	result := map[string]interface{}{
		"model":    model,
		"messages": messages,
	}

	if stream {
		result["stream"] = true
		result["stream_options"] = map[string]interface{}{"include_usage": true}
	}

	// DeepSeek uses max_completion_tokens instead of max_tokens
	if req.MaxTokens > 0 {
		result["max_completion_tokens"] = req.MaxTokens
	}

	// Temperature: 0 when not reasoning, omitted when reasoning
	isReasoning := req.Thinking != nil && req.Thinking.BudgetTokens > 0
	if !isReasoning {
		result["temperature"] = 0
	}

	if req.TopP > 0 {
		result["top_p"] = req.TopP
	}
	if len(req.Stop) > 0 {
		result["stop"] = req.Stop
	}

	// Tools with strict: true
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
			tools = append(tools, map[string]interface{}{
				"type": "function",
				"function": map[string]interface{}{
					"name":        tool.Name,
					"description": tool.Description,
					"parameters":  inputSchema,
					"strict":      true,
				},
			})
		}
		if len(tools) > 0 {
			result["tools"] = tools
			result["tool_choice"] = "auto"
		}
	}

	return result
}

// addReasoningContent extracts thinking blocks from the original messages
// and adds them as reasoning_content on the corresponding OpenAI-format assistant messages.
// Replicates addReasoningContent from r1-format.ts.
func (h *DeepSeekHandler) addReasoningContent(openAIMessages []map[string]interface{}, req *Request) []map[string]interface{} {
	// Build a map: message index → reasoning_content from thinking blocks
	reasoningByIndex := map[int]string{}

	msgIdx := 0
	if req.System != "" {
		msgIdx++ // system message is at index 0
	}

	for _, msg := range req.Messages {
		currentIdx := msgIdx
		msgIdx++

		if msg.Role != "assistant" {
			continue
		}
		if len(msg.ContentBlocks) == 0 {
			continue
		}

		var reasoning string
		for _, block := range msg.ContentBlocks {
			if block.Type == "thinking" {
				reasoning += block.Thinking
			}
		}
		if reasoning != "" {
			reasoningByIndex[currentIdx] = reasoning
		}
	}

	// Apply reasoning_content to matching OpenAI messages
	for idx, reasoning := range reasoningByIndex {
		if idx < len(openAIMessages) {
			role, _ := openAIMessages[idx]["role"].(string)
			if role == "assistant" {
				openAIMessages[idx]["reasoning_content"] = reasoning
			}
		}
	}

	return openAIMessages
}

// coerceAssistantMessages ensures all assistant messages have non-null content
// and non-null reasoning_content, as required by DeepSeek.
func (h *DeepSeekHandler) coerceAssistantMessages(messages []map[string]interface{}) []map[string]interface{} {
	for _, msg := range messages {
		role, _ := msg["role"].(string)
		if role != "assistant" {
			continue
		}
		// content must not be null
		if msg["content"] == nil {
			msg["content"] = ""
		}
		// reasoning_content must be present (default empty string)
		if _, ok := msg["reasoning_content"]; !ok {
			msg["reasoning_content"] = ""
		}
	}
	return messages
}

func (h *DeepSeekHandler) parseSSEStream(body io.Reader, callback func(StreamChunk) error) error {
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
				PromptTokens           int `json:"prompt_tokens"`
				CompletionTokens       int `json:"completion_tokens"`
				PromptCacheHitTokens   int `json:"prompt_cache_hit_tokens"`
				PromptCacheMissTokens  int `json:"prompt_cache_miss_tokens"`
			} `json:"usage"`
		}

		if err := json.Unmarshal([]byte(data), &chunk); err != nil {
			continue
		}

		if len(chunk.Choices) == 0 {
			// Usage-only chunk
			if chunk.Usage.PromptTokens > 0 || chunk.Usage.CompletionTokens > 0 {
				usage := &Usage{
					InputTokens:              chunk.Usage.PromptTokens,
					OutputTokens:             chunk.Usage.CompletionTokens,
					CacheReadInputTokens:     chunk.Usage.PromptCacheHitTokens,
					CacheCreationInputTokens: chunk.Usage.PromptCacheMissTokens,
				}
				callback(StreamChunk{Type: "stop", Usage: usage})
			}
			continue
		}

		choice := chunk.Choices[0]
		delta := choice.Delta

		if delta.Content != "" {
			callback(StreamChunk{Type: "delta", TextDelta: delta.Content})
		}

		// reasoning_content — DeepSeek's thinking output
		if delta.ReasoningContent != "" {
			callback(StreamChunk{Type: "delta", Thinking: delta.ReasoningContent})
		}

		// Tool calls
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
			if state.id != "" && state.name != "" && tc.Function.Arguments != "" {
				callback(StreamChunk{
					Type:      "delta",
					Index:     idx,
					JSONDelta: tc.Function.Arguments,
					ToolCallID:   state.id,
					ToolCallName: state.name,
				})
			}
		}

		if choice.FinishReason != nil {
			finishReason := *choice.FinishReason
			usage := &Usage{}
			if chunk.Usage.PromptTokens > 0 || chunk.Usage.CompletionTokens > 0 {
				usage.InputTokens = chunk.Usage.PromptTokens
				usage.OutputTokens = chunk.Usage.CompletionTokens
				usage.CacheReadInputTokens = chunk.Usage.PromptCacheHitTokens
				usage.CacheCreationInputTokens = chunk.Usage.PromptCacheMissTokens
			}
			callback(StreamChunk{Type: "stop", FinishReason: finishReason, Usage: usage})
		}
	}

	return nil
}

func (h *DeepSeekHandler) convertResponse(resp map[string]interface{}) *SendResult {
	var content []ContentBlock

	if choices, ok := resp["choices"].([]interface{}); ok && len(choices) > 0 {
		if choice, ok := choices[0].(map[string]interface{}); ok {
			if msg, ok := choice["message"].(map[string]interface{}); ok {
				if text, ok := msg["content"].(string); ok && text != "" {
					content = append(content, ContentBlock{Type: "text", Text: text})
				}
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
		if tokens, ok := usageMap["prompt_cache_hit_tokens"].(float64); ok {
			usage.CacheReadInputTokens = int(tokens)
		}
		if tokens, ok := usageMap["prompt_cache_miss_tokens"].(float64); ok {
			usage.CacheCreationInputTokens = int(tokens)
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

	return &SendResult{
		Content:    content,
		Usage:      usage,
		StopReason: stopReason,
	}
}
