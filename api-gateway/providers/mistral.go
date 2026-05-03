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

// MistralHandler handles Mistral API requests.
// Mistral uses an OpenAI-compatible chat completions API with:
//   - User messages: string content or array of text/image_url parts
//   - Assistant messages: text-only (string content)
//   - tool_choice: "any" when tools present
//   - Temperature hardcoded to 0
//   - SSE streaming with tool_calls accumulator
type MistralHandler struct {
	httpClient *http.Client
	baseURL    string
	apiKey     string
}

func NewMistralHandler() *MistralHandler {
	return &MistralHandler{
		httpClient: &http.Client{},
		baseURL:    "https://api.mistral.ai/v1",
	}
}

func (h *MistralHandler) getConfig(req *Request) (baseURL, apiKey string) {
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

func (h *MistralHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.getConfig(req)
	mistralReq := h.buildRequest(req, false)

	reqBody, err := json.Marshal(mistralReq)
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

func (h *MistralHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.getConfig(req)
	mistralReq := h.buildRequest(req, true)

	reqBody, err := json.Marshal(mistralReq)
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

func (h *MistralHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
}

func (h *MistralHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	messages := h.convertMessages(req)

	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = "devstral-2512"
	}

	result := map[string]interface{}{
		"model":       model,
		"messages":    messages,
		"temperature": 0,
	}

	if stream {
		result["stream"] = true
	}

	if req.MaxTokens > 0 {
		result["max_tokens"] = req.MaxTokens
	}
	if req.TopP > 0 {
		result["top_p"] = req.TopP
	}

	// Tools with tool_choice: "any"
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
				},
			})
		}
		if len(tools) > 0 {
			result["tools"] = tools
			result["tool_choice"] = "any"
		}
	}

	return result
}

// convertMessages converts internal messages to Mistral format.
// User messages: text/image parts array or string.
// Assistant messages: text-only string (content blocks joined with \n).
func (h *MistralHandler) convertMessages(req *Request) []map[string]interface{} {
	var messages []map[string]interface{}

	// System prompt as first message
	if req.System != "" {
		messages = append(messages, map[string]interface{}{
			"role":    "system",
			"content": req.System,
		})
	}

	for _, msg := range req.Messages {
		switch msg.Role {
		case "user":
			messages = append(messages, h.convertUserMessage(msg))
		case "assistant":
			messages = append(messages, h.convertAssistantMessage(msg))
		case "tool":
			messages = append(messages, map[string]interface{}{
				"role":         "tool",
				"tool_call_id": msg.ToolUseID,
				"content":      msg.Content,
			})
		}
	}

	return messages
}

func (h *MistralHandler) convertUserMessage(msg Message) map[string]interface{} {
	if len(msg.ContentBlocks) == 0 {
		return map[string]interface{}{
			"role":    "user",
			"content": msg.Content,
		}
	}

	// Filter to text and image blocks only
	var parts []map[string]interface{}
	for _, block := range msg.ContentBlocks {
		switch block.Type {
		case "text":
			parts = append(parts, map[string]interface{}{
				"type": "text",
				"text": block.Text,
			})
		case "image":
			if block.ImageSource != nil {
				url := ""
				if block.ImageSource.Data != "" {
					url = "data:" + block.ImageSource.MimeType + ";base64," + block.ImageSource.Data
				} else if block.ImageSource.URL != "" {
					url = block.ImageSource.URL
				}
				if url != "" {
					parts = append(parts, map[string]interface{}{
						"type":      "image_url",
						"image_url": map[string]interface{}{"url": url},
					})
				}
			}
		}
	}

	if len(parts) == 0 {
		return map[string]interface{}{
			"role":    "user",
			"content": msg.Content,
		}
	}

	return map[string]interface{}{
		"role":    "user",
		"content": parts,
	}
}

// convertAssistantMessage returns a text-only assistant message.
// Mistral assistant messages only support string content.
func (h *MistralHandler) convertAssistantMessage(msg Message) map[string]interface{} {
	result := map[string]interface{}{
		"role": "assistant",
	}

	// Extract text content
	var textParts []string
	if msg.Content != "" {
		textParts = append(textParts, msg.Content)
	}
	for _, block := range msg.ContentBlocks {
		if block.Type == "text" {
			textParts = append(textParts, block.Text)
		}
	}
	result["content"] = strings.Join(textParts, "\n")

	// Tool calls
	var toolCalls []map[string]interface{}
	for _, block := range msg.ContentBlocks {
		if block.Type == "tool_use" && block.ToolUse != nil {
			toolCalls = append(toolCalls, map[string]interface{}{
				"id":   block.ToolUse.ID,
				"type": "function",
				"function": map[string]interface{}{
					"name":      block.ToolUse.Function.Name,
					"arguments": block.ToolUse.Function.Arguments,
				},
			})
		}
	}
	// Also check legacy ToolCalls field
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
	if len(toolCalls) > 0 {
		result["tool_calls"] = toolCalls
	}

	return result
}

func (h *MistralHandler) parseSSEStream(body io.Reader, callback func(StreamChunk) error) error {
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
					Content   interface{} `json:"content"`
					ToolCalls []struct {
						Index    int    `json:"index"`
						ID       string `json:"id"`
						Function struct {
							Name      string      `json:"name"`
							Arguments interface{} `json:"arguments"`
						} `json:"function"`
					} `json:"tool_calls"`
				} `json:"delta"`
				FinishReason *string `json:"finish_reason"`
			} `json:"choices"`
			Usage struct {
				PromptTokens     int `json:"prompt_tokens"`
				CompletionTokens int `json:"completion_tokens"`
			} `json:"usage"`
		}

		if err := json.Unmarshal([]byte(data), &chunk); err != nil {
			continue
		}

		if len(chunk.Choices) == 0 {
			if chunk.Usage.PromptTokens > 0 || chunk.Usage.CompletionTokens > 0 {
				usage := &Usage{
					InputTokens:  chunk.Usage.PromptTokens,
					OutputTokens: chunk.Usage.CompletionTokens,
				}
				callback(StreamChunk{Type: "stop", Usage: usage})
			}
			continue
		}

		choice := chunk.Choices[0]
		delta := choice.Delta

		// Content — Mistral returns string or array of {type, text}
		if delta.Content != nil {
			content := extractMistralContent(delta.Content)
			if content != "" {
				callback(StreamChunk{Type: "delta", TextDelta: content})
			}
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
			args := ""
			switch v := tc.Function.Arguments.(type) {
			case string:
				args = v
			default:
				b, _ := json.Marshal(v)
				args = string(b)
			}
			if state.id != "" && state.name != "" && args != "" {
				callback(StreamChunk{
					Type:      "delta",
					Index:     idx,
					JSONDelta: args,
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
			}
			callback(StreamChunk{Type: "stop", FinishReason: finishReason, Usage: usage})
		}
	}

	return nil
}

// extractMistralContent handles Mistral's content which can be
// a string or an array of {type: "text", text: "..."} objects.
func extractMistralContent(raw interface{}) string {
	switch v := raw.(type) {
	case string:
		return v
	case []interface{}:
		var parts []string
		for _, item := range v {
			if m, ok := item.(map[string]interface{}); ok {
				if t, _ := m["type"].(string); t == "text" {
					if text, _ := m["text"].(string); text != "" {
						parts = append(parts, text)
					}
				}
			}
		}
		return strings.Join(parts, "")
	default:
		return ""
	}
}

func (h *MistralHandler) convertResponse(resp map[string]interface{}) *SendResult {
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

// Ensure MistralHandler satisfies Handler
var _ Handler = (*MistralHandler)(nil)
