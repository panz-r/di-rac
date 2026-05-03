package providers

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"strings"
)

// OpenAICompatConfig configures an OpenAI-compatible provider.
// Fill in only what differs from defaults; the rest are zero-valued.
type OpenAICompatConfig struct {
	BaseURL             string
	DefaultModel        string
	MaxCompletionTokens bool   // true = use "max_completion_tokens" instead of "max_tokens"
	Temperature         *float64 // nil = use 0, else use this value; set to sentinel -1 to omit entirely
	ToolChoice          string   // "" or "auto" (default) or "any"
	NoStreamOptions     bool     // true = skip stream_options.include_usage
	ExtraHeaders        map[string]string
	// ModifyRequest is called after the standard request is built.
	// Use it to add provider-specific params (e.g. reasoning_format, drop_params).
	ModifyRequest func(req *Request, result map[string]interface{})
	// ModifyMessages is called on the converted messages before building the request.
	// Use it for R1-format transforms, addReasoningContent, etc.
	ModifyMessages func(messages []map[string]interface{}, req *Request) []map[string]interface{}
	// FinishReasonMap maps non-standard finish reasons (e.g. ZAI's "model_context_window_exceeded").
	FinishReasonMap func(string) string
}

// openaiCompatHandler implements Handler for any OpenAI-compatible API.
type openaiCompatHandler struct {
	httpClient *http.Client
	config     OpenAICompatConfig
}

func newOpenAICompatHandler(config OpenAICompatConfig) *openaiCompatHandler {
	return &openaiCompatHandler{
		httpClient: &http.Client{},
		config:     config,
	}
}

func (h *openaiCompatHandler) getConfig(req *Request) (baseURL, apiKey string) {
	return h.config.BaseURL, ""
}

func (h *openaiCompatHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.resolveConfig(req)
	payload := h.buildRequest(req, false)

	reqBody, err := json.Marshal(payload)
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

	var raw map[string]interface{}
	if err := json.Unmarshal(body, &raw); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}
	return openaiConvertResponse(raw, h.config.FinishReasonMap), nil
}

func (h *openaiCompatHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.resolveConfig(req)
	payload := h.buildRequest(req, true)

	reqBody, err := json.Marshal(payload)
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

	return openaiParseSSE(resp.Body, callback, h.config.FinishReasonMap)
}

func (h *openaiCompatHandler) resolveConfig(req *Request) (baseURL, apiKey string) {
	baseURL = h.config.BaseURL
	apiKey = ""
	if req.Provider.BaseURL != "" {
		baseURL = req.Provider.BaseURL
	}
	if req.Provider.APIKey != "" {
		apiKey = req.Provider.APIKey
	}
	return
}

func (h *openaiCompatHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
	for k, v := range h.config.ExtraHeaders {
		httpReq.Header.Set(k, v)
	}
}

func (h *openaiCompatHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	messages := openaiConvertMessages(req)

	if h.config.ModifyMessages != nil {
		messages = h.config.ModifyMessages(messages, req)
	}

	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = h.config.DefaultModel
	}

	result := map[string]interface{}{
		"model":    model,
		"messages": messages,
	}

	// Temperature
	if h.config.Temperature != nil {
		if *h.config.Temperature >= 0 {
			result["temperature"] = *h.config.Temperature
		}
		// sentinel -1 = omit temperature entirely
	} else {
		result["temperature"] = 0
	}

	if stream && !h.config.NoStreamOptions {
		result["stream"] = true
		result["stream_options"] = map[string]interface{}{"include_usage": true}
	} else if stream {
		result["stream"] = true
	}

	// Max tokens
	if req.MaxTokens > 0 {
		if h.config.MaxCompletionTokens {
			result["max_completion_tokens"] = req.MaxTokens
		} else {
			result["max_tokens"] = req.MaxTokens
		}
	}
	if req.TopP > 0 {
		result["top_p"] = req.TopP
	}
	if len(req.Stop) > 0 {
		result["stop"] = req.Stop
	}

	// Tools
	if len(req.Tools) > 0 {
		tools := openaiBuildTools(req.Tools)
		if len(tools) > 0 {
			result["tools"] = tools
			choice := h.config.ToolChoice
			if choice == "" {
				choice = "auto"
			}
			result["tool_choice"] = choice
		}
	}

	// Provider-specific modifications
	if h.config.ModifyRequest != nil {
		h.config.ModifyRequest(req, result)
	}

	return result
}

// --- Shared helpers ---

// openaiConvertMessages converts DiracStorageMessage-format messages to OpenAI chat completion format.
func openaiConvertMessages(req *Request) []map[string]interface{} {
	var messages []map[string]interface{}

	if req.System != "" {
		messages = append(messages, map[string]interface{}{
			"role":    "system",
			"content": req.System,
		})
	}

	for _, msg := range req.Messages {
		if len(msg.ContentBlocks) > 0 {
			messages = openaiConvertContentBlockMessage(messages, msg)
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

func openaiConvertContentBlockMessage(messages []map[string]interface{}, msg Message) []map[string]interface{} {
	if msg.Role == "user" {
		return openaiConvertUserContentBlocks(messages, msg)
	}
	return openaiConvertAssistantContentBlocks(messages, msg)
}

func openaiConvertUserContentBlocks(messages []map[string]interface{}, msg Message) []map[string]interface{} {
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

	for _, block := range toolResultParts {
		if block.ToolResult == nil {
			continue
		}
		messages = append(messages, map[string]interface{}{
			"role":         "tool",
			"tool_call_id": block.ToolResult.ToolUseID,
			"content":      block.ToolResult.Content,
		})
	}

	content := append(textParts, imageParts...)
	if len(content) > 0 {
		messages = append(messages, map[string]interface{}{
			"role":    "user",
			"content": content,
		})
	}

	return messages
}

func openaiConvertAssistantContentBlocks(messages []map[string]interface{}, msg Message) []map[string]interface{} {
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

	m := map[string]interface{}{"role": "assistant"}
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

// openaiBuildTools parses raw tool JSON into OpenAI-format tool definitions.
func openaiBuildTools(toolsRaw []json.RawMessage) []map[string]interface{} {
	var tools []map[string]interface{}
	for i, toolJSON := range toolsRaw {
		var tool struct {
			Name        string          `json:"name"`
			Description string          `json:"description"`
			InputSchema json.RawMessage `json:"input_schema"`
		}
		if err := json.Unmarshal(toolJSON, &tool); err != nil {
			log.Printf("[openaiBuildTools] tool[%d] unmarshal failed: %v (raw: %s)", i, err, string(toolJSON[:min(len(toolJSON), 100)]))
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
	log.Printf("[openaiBuildTools] parsed %d/%d tools: %v", len(tools), len(toolsRaw), func() []string {
		var names []string
		for _, t := range tools {
			if fn, ok := t["function"].(map[string]interface{}); ok {
				names = append(names, fn["name"].(string))
			}
		}
		return names
	}())
	return tools
}

// openaiAddReasoningContent extracts thinking blocks from original messages
// and injects them as reasoning_content on assistant messages.
func openaiAddReasoningContent(messages []map[string]interface{}, req *Request) []map[string]interface{} {
	reasoningByIndex := map[int]string{}
	msgIdx := 0
	if req.System != "" {
		msgIdx++
	}
	for _, msg := range req.Messages {
		currentIdx := msgIdx
		msgIdx++
		if msg.Role != "assistant" || len(msg.ContentBlocks) == 0 {
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
	for idx, reasoning := range reasoningByIndex {
		if idx < len(messages) {
			role, _ := messages[idx]["role"].(string)
			if role == "assistant" {
				messages[idx]["reasoning_content"] = reasoning
			}
		}
	}
	return messages
}

// openaiParseSSE reads an SSE stream and emits StreamChunks.
// Handles all known OpenAI-compatible fields across providers:
// content, reasoning_content, tool_calls, finish_reason, usage with all cache variants.
func openaiParseSSE(body io.Reader, callback func(StreamChunk) error, finishReasonMap func(string) string) error {
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
					ReasoningDetails []struct {
						Text string `json:"text"`
					} `json:"reasoning_details"`
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
				// OpenAI / Groq / xAI
				PromptTokensDetails struct {
					CachedTokens int `json:"cached_tokens"`
				} `json:"prompt_tokens_details"`
				// DeepSeek / Qwen / Fireworks
				PromptCacheHitTokens  int `json:"prompt_cache_hit_tokens"`
				PromptCacheMissTokens int `json:"prompt_cache_miss_tokens"`
				// Moonshot top-level
				CachedTokens int `json:"cached_tokens"`
				// ZAI
				CompletionTokensDetails struct {
					ReasoningTokens int `json:"reasoning_tokens"`
				} `json:"completion_tokens_details"`
			} `json:"usage"`
		}

		if err := json.Unmarshal([]byte(data), &chunk); err != nil {
			continue
		}

		usage := openaiExtractUsage(chunk.Usage)

		if len(chunk.Choices) == 0 {
			if usage != nil {
				callback(StreamChunk{Type: "stop", Usage: usage})
			}
			continue
		}

		choice := chunk.Choices[0]
		delta := choice.Delta

		if delta.Content != "" {
			callback(StreamChunk{Type: "delta", TextDelta: delta.Content})
		}
		if delta.ReasoningContent != "" {
			callback(StreamChunk{Type: "delta", Thinking: delta.ReasoningContent})
		}
		// Groq reasoning field (DeepSeek models with reasoning_format: "parsed")
		if delta.Reasoning != "" {
			callback(StreamChunk{Type: "delta", Thinking: delta.Reasoning})
			// MiniMax reasoning_details field (with reasoning_split=true)
			for _, rd := range delta.ReasoningDetails {
				if rd.Text != "" {
					callback(StreamChunk{Type: "delta", Thinking: rd.Text})
				}
			}
		}

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
			// Log every tool call chunk for debugging
			log.Printf("[SSE] tool_call: idx=%d id=%q name=%q args_len=%d args=%q", idx, tc.ID, tc.Function.Name, len(tc.Function.Arguments), tc.Function.Arguments)
			// Emit tool call delta whenever we have arguments and a name.
			// OpenAI streams send id+name first, then argument fragments in separate chunks.
			if tc.Function.Arguments != "" {
				callback(StreamChunk{
					Type:         "delta",
					Index:        idx,
					JSONDelta:    tc.Function.Arguments,
					ToolCallID:   state.id,
					ToolCallName: state.name,
				})
			}
		}

		if choice.FinishReason != nil {
			fr := *choice.FinishReason
			if finishReasonMap != nil {
				fr = finishReasonMap(fr)
			}
			callback(StreamChunk{Type: "stop", FinishReason: fr, Usage: usage})
		}
	}

	return nil
}

// openaiExtractUsage builds a Usage from the unified SSE usage struct.
func openaiExtractUsage(u struct {
	PromptTokens     int `json:"prompt_tokens"`
	CompletionTokens int `json:"completion_tokens"`
	PromptTokensDetails struct {
		CachedTokens int `json:"cached_tokens"`
	} `json:"prompt_tokens_details"`
	PromptCacheHitTokens  int `json:"prompt_cache_hit_tokens"`
	PromptCacheMissTokens int `json:"prompt_cache_miss_tokens"`
	CachedTokens          int `json:"cached_tokens"`
	CompletionTokensDetails struct {
		ReasoningTokens int `json:"reasoning_tokens"`
	} `json:"completion_tokens_details"`
}) *Usage {
	if u.PromptTokens == 0 && u.CompletionTokens == 0 {
		return nil
	}
	return &Usage{
		InputTokens:              u.PromptTokens,
		OutputTokens:             u.CompletionTokens,
		CacheReadInputTokens:     u.PromptTokensDetails.CachedTokens + u.PromptCacheHitTokens + u.CachedTokens,
		CacheCreationInputTokens: u.PromptCacheMissTokens,
		ReasoningTokens:          u.CompletionTokensDetails.ReasoningTokens,
	}
}

// openaiConvertResponse parses a non-streaming OpenAI-format response.
func openaiConvertResponse(resp map[string]interface{}, finishReasonMap func(string) string) *SendResult {
	var content []ContentBlock

	if choices, ok := resp["choices"].([]interface{}); ok && len(choices) > 0 {
		if choice, ok := choices[0].(map[string]interface{}); ok {
			if msg, ok := choice["message"].(map[string]interface{}); ok {
				if text, ok := msg["content"].(string); ok && text != "" {
					content = append(content, ContentBlock{Type: "text", Text: text})
				}
				// MiniMax reasoning_details (with reasoning_split=true)
				if rds, ok := msg["reasoning_details"].([]interface{}); ok {
					for _, rd := range rds {
						if rdMap, ok := rd.(map[string]interface{}); ok {
							if text, ok := rdMap["text"].(string); ok && text != "" {
								content = append(content, ContentBlock{Type: "thinking", Thinking: text})
							}
						}
					}
				}
				// Standard reasoning_content (DeepSeek, Groq, etc.)
				if rc, ok := msg["reasoning_content"].(string); ok && rc != "" {
					content = append(content, ContentBlock{Type: "thinking", Thinking: rc})
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
			}
		}
		if hit, ok := usageMap["prompt_cache_hit_tokens"].(float64); ok {
			usage.CacheReadInputTokens = int(hit)
		}
		if miss, ok := usageMap["prompt_cache_miss_tokens"].(float64); ok {
			usage.CacheCreationInputTokens = int(miss)
		}
	}

	stopReason := ""
	if choices, ok := resp["choices"].([]interface{}); ok && len(choices) > 0 {
		if choice, ok := choices[0].(map[string]interface{}); ok {
			if sr, ok := choice["finish_reason"].(string); ok {
				if finishReasonMap != nil {
					stopReason = finishReasonMap(sr)
				} else {
					stopReason = sr
				}
			}
		}
	}

	return &SendResult{
		Content:    content,
		Usage:      usage,
		StopReason: stopReason,
	}
}
