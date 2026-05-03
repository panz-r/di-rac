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

// GroqHandler handles Groq API requests.
// Groq uses an OpenAI-compatible chat completions API with:
//   - Base URL: https://api.groq.com/openai/v1
//   - Model family detection for special params (e.g. DeepSeek: reasoning_format, top_p)
//   - reasoning field in streaming deltas (for reasoning models)
//   - prompt_tokens_details.cached_tokens for cache reads
//   - Temperature hardcoded to 0
//   - max_tokens (not max_completion_tokens)
type GroqHandler struct {
	httpClient *http.Client
	baseURL    string
	apiKey     string
}

func NewGroqHandler() *GroqHandler {
	return &GroqHandler{
		httpClient: &http.Client{},
		baseURL:    "https://api.groq.com/openai/v1",
	}
}

func (h *GroqHandler) getConfig(req *Request) (baseURL, apiKey string) {
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

func (h *GroqHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.getConfig(req)
	groqReq := h.buildRequest(req, false)

	reqBody, err := json.Marshal(groqReq)
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

func (h *GroqHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.getConfig(req)
	groqReq := h.buildRequest(req, true)

	reqBody, err := json.Marshal(groqReq)
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

func (h *GroqHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
}

func (h *GroqHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	// Reuse OpenAI message conversion
	openai := &OpenAIHandler{}
	messages := openai.convertMessages(req)

	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = "moonshotai/kimi-k2-instruct-0905"
	}

	result := map[string]interface{}{
		"model":       model,
		"messages":    messages,
		"temperature": 0,
	}

	if stream {
		result["stream"] = true
		result["stream_options"] = map[string]interface{}{"include_usage": true}
	}

	if req.MaxTokens > 0 {
		result["max_tokens"] = req.MaxTokens
	}
	if req.TopP > 0 {
		result["top_p"] = req.TopP
	}

	// Model family special params
	family := detectGroqModelFamily(model)
	if family.specialParams != nil {
		for k, v := range family.specialParams {
			// Don't override user-specified values
			if _, exists := result[k]; !exists {
				result[k] = v
			}
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
			result["tool_choice"] = "auto"
		}
	}

	return result
}

// groqModelFamily represents a Groq model family with special behavior.
type groqModelFamily struct {
	name          string
	specialParams map[string]interface{}
}

func detectGroqModelFamily(modelID string) groqModelFamily {
	switch {
	case strings.Contains(modelID, "deepseek"):
		return groqModelFamily{
			name: "DeepSeek",
			specialParams: map[string]interface{}{
				"top_p":            0.95,
				"reasoning_format": "parsed",
			},
		}
	default:
		return groqModelFamily{name: "default"}
	}
}

func (h *GroqHandler) parseSSEStream(body io.Reader, callback func(StreamChunk) error) error {
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
					Reasoning        string `json:"reasoning"`
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
			if chunk.Usage.PromptTokens > 0 || chunk.Usage.CompletionTokens > 0 {
				usage := &Usage{
					InputTokens:          chunk.Usage.PromptTokens - chunk.Usage.PromptTokensDetails.CachedTokens,
					OutputTokens:         chunk.Usage.CompletionTokens,
					CacheReadInputTokens: chunk.Usage.PromptTokensDetails.CachedTokens,
				}
				callback(StreamChunk{Type: "stop", Usage: usage})
			}
			continue
		}

		choice := chunk.Choices[0]
		delta := choice.Delta

		// Groq reasoning field (DeepSeek models with reasoning_format: "parsed")
		if delta.Reasoning != "" {
			callback(StreamChunk{Type: "delta", Thinking: delta.Reasoning})
		}

		// Standard reasoning_content
		if delta.ReasoningContent != "" {
			callback(StreamChunk{Type: "delta", Thinking: delta.ReasoningContent})
		}

		if delta.Content != "" {
			callback(StreamChunk{Type: "delta", TextDelta: delta.Content})
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
				usage.InputTokens = chunk.Usage.PromptTokens - chunk.Usage.PromptTokensDetails.CachedTokens
				usage.OutputTokens = chunk.Usage.CompletionTokens
				usage.CacheReadInputTokens = chunk.Usage.PromptTokensDetails.CachedTokens
			}
			callback(StreamChunk{Type: "stop", FinishReason: finishReason, Usage: usage})
		}
	}

	return nil
}

func (h *GroqHandler) convertResponse(resp map[string]interface{}) *SendResult {
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
		if details, ok := usageMap["prompt_tokens_details"].(map[string]interface{}); ok {
			if cached, ok := details["cached_tokens"].(float64); ok {
				usage.CacheReadInputTokens = int(cached)
				usage.InputTokens -= int(cached)
			}
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

// Ensure GroqHandler satisfies Handler
var _ Handler = (*GroqHandler)(nil)
