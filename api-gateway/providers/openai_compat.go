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
	"sort"
	"strconv"
	"strings"
	"time"
)

const maxBodySize = 10 << 20 // 10 MB safety cap on response bodies

// contextReader wraps an io.Reader so that Read respects context cancellation.
// Without this, a blocked read on an HTTP response body can outlive context
// cancellation, leaking goroutines until the provider closes the connection.
type contextReader struct {
	ctx context.Context
	r   io.Reader
}

func (cr *contextReader) Read(p []byte) (n int, err error) {
	select {
	case <-cr.ctx.Done():
		return 0, cr.ctx.Err()
	default:
	}
	return cr.r.Read(p)
}

// OpenAICompatConfig configures an OpenAI-compatible provider.
// Fill in only what differs from defaults; the rest are zero-valued.
type OpenAICompatConfig struct {
	BaseURL             string
	DefaultModel        string
	MaxCompletionTokens bool   // true = use "max_completion_tokens" instead of "max_tokens"
	Temperature         *float64 // nil = use 0, else use this value; set to sentinel -1 to omit entirely
	ToolChoice          string   // "" or "auto" (default) or "any"
	NoStreamOptions     bool     // true = skip stream_options.include_usage
	ContentArraySupport bool     // true = delta.content may be [{type:"text",text:"..."}] instead of string
	StrictTools         bool     // true = add "strict": true to all tool function definitions
	ExtraHeaders        map[string]string
	// ModifyRequest is called after the standard request is built.
	// Use it to add provider-specific params (e.g. reasoning_format, drop_params).
	ModifyRequest func(req *Request, result map[string]interface{})
	// ModifyHeaders is called after standard headers are set.
	// Use it to add per-request headers based on settings (e.g. Catalyst proxy).
	ModifyHeaders func(httpReq *http.Request, req *Request)
	// ModifyMessages is called on the converted messages before building the request.
	// Use it for R1-format transforms, addReasoningContent, etc.
	ModifyMessages func(messages []map[string]interface{}, req *Request) []map[string]interface{}
	// FinishReasonMap maps non-standard finish reasons (e.g. ZAI's "model_context_window_exceeded").
	FinishReasonMap func(string) string
	// Capabilities declares this provider's supported settings and features.
	Capabilities *ProviderInfo
}

// openaiCompatHandler implements Handler for any OpenAI-compatible API.
type openaiCompatHandler struct {
	httpClient *http.Client
	config     OpenAICompatConfig
}

func newOpenAICompatHandler(config OpenAICompatConfig) *openaiCompatHandler {
	return &openaiCompatHandler{
		httpClient: SharedHTTPClient,
		config:     config,
	}
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
	h.setHeaders(httpReq, apiKey, req)

	resp, err := h.httpClient.Do(httpReq)
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

	var raw map[string]interface{}
	if err := json.Unmarshal(body, &raw); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}
	result := openaiConvertResponse(raw, h.config.FinishReasonMap)
	result.Raw = body
	if model, _ := raw["model"].(string); model != "" {
		result.Model = model
	}
	return result, nil
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
	h.setHeaders(httpReq, apiKey, req)
	httpReq.Header.Set("Accept", "text/event-stream")
	httpReq.Header.Set("Cache-Control", "no-cache")

	resp, err := h.httpClient.Do(httpReq)
	if err != nil {
		return wrapTransientError(fmt.Errorf("request failed: %w", err))
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
		log.Printf("[openai-compat] %s %s → status %d: %s", httpReq.Method, httpReq.URL.Path, resp.StatusCode, string(body))
		return newAPIErrorFromResp(resp, string(body))
	}

	return openaiParseSSE(ctx, &contextReader{ctx: ctx, r: resp.Body}, callback, h.config.FinishReasonMap, h.config.ContentArraySupport)
}

func (h *openaiCompatHandler) Capabilities() *ProviderInfo {
	return h.config.Capabilities
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

// newAPIError creates a ProviderAPIError with ContextExceeded auto-detected
// from the response body. Each provider's error format is checked here so
// the detection stays in the provider layer.
func newAPIError(statusCode int, body string) *ProviderAPIError {
	ctxExceeded := false
	if statusCode == 400 || statusCode == 413 {
		lower := strings.ToLower(body)
		ctxExceeded = strings.Contains(lower, "context window") ||
			strings.Contains(lower, "context_length_exceeded") ||
			strings.Contains(lower, "maximum context length") ||
			strings.Contains(lower, "token limit") ||
			strings.Contains(lower, "input is too long")
	}
	return &ProviderAPIError{
		StatusCode:      statusCode,
		Message:         fmt.Sprintf("API error (status %d): %s", statusCode, body),
		Retriable:       statusCode == 429,
		ContextExceeded: ctxExceeded,
	}
}

// newAPIErrorFromResp creates a ProviderAPIError from an HTTP response,
// parsing Retry-After header when present.
func newAPIErrorFromResp(resp *http.Response, body string) *ProviderAPIError {
	pae := newAPIError(resp.StatusCode, body)
	if pae.StatusCode == 429 {
		if ra := resp.Header.Get("Retry-After"); ra != "" {
			if secs, err := strconv.Atoi(ra); err == nil && secs > 0 {
				pae.RetryAfter = time.Duration(secs) * time.Second
			}
		}
	}
	return pae
}

func (h *openaiCompatHandler) setHeaders(httpReq *http.Request, apiKey string, req *Request) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
	for k, v := range h.config.ExtraHeaders {
		httpReq.Header.Set(k, v)
	}
	if h.config.ModifyHeaders != nil {
		h.config.ModifyHeaders(httpReq, req)
	}
}

func (h *openaiCompatHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	messages := openaiConvertMessages(req)

	// Ensure assistant messages with tool_calls have non-nil content.
	// Some providers (Groq, Together, etc.) reject nil content.
	for _, m := range messages {
		if m["role"] == "assistant" {
			if _, hasToolCalls := m["tool_calls"]; hasToolCalls {
				if m["content"] == nil {
					m["content"] = ""
				}
			}
		}
	}

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

	// Temperature: only set if config provides an explicit override.
	// Individual providers set temperature via ModifyRequest using
	// SettingFloat/SettingIsNull to respect user intent vs provider defaults.
	if h.config.Temperature != nil {
		if *h.config.Temperature >= 0 {
			result["temperature"] = *h.config.Temperature
		}
		// sentinel -1 = omit temperature entirely
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
		tools := openaiBuildTools(req.Tools, h.config.StrictTools)
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

// openaiConvertMessages converts di-vrr storage message-format messages to OpenAI chat completion format.
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

	return sanitizeOrphanedToolMessages(messages)
}

// sanitizeOrphanedToolMessages detects "tool" role messages that have no matching
// preceding assistant message with tool_calls, and converts them to user role.
// This can happen when a client compresses conversation history and drops the
// assistant tool-call messages but keeps the tool results.
func sanitizeOrphanedToolMessages(messages []map[string]interface{}) []map[string]interface{} {
	knownToolIDs := map[string]bool{}

	for _, m := range messages {
		role, _ := m["role"].(string)
		if role == "assistant" {
			if tcs, ok := m["tool_calls"].([]map[string]interface{}); ok {
				for _, tc := range tcs {
					if id, ok := tc["id"].(string); ok && id != "" {
						knownToolIDs[id] = true
					}
				}
			}
		}
	}

	var cleaned []map[string]interface{}
	orphanCount := 0
	for _, m := range messages {
		role, _ := m["role"].(string)
		if role == "tool" {
			id, _ := m["tool_call_id"].(string)
			if id == "" || !knownToolIDs[id] {
				orphanCount++
				content, _ := m["content"].(string)
				cleaned = append(cleaned, map[string]interface{}{
					"role":    "user",
					"content": fmt.Sprintf("[Tool result for %s]: %s", id, content),
				})
				continue
			}
		}
		cleaned = append(cleaned, m)
	}

	if orphanCount > 0 {
		log.Printf("[openai-compat] sanitized %d orphaned tool messages (missing assistant tool_calls)", orphanCount)
	}

	return cleaned
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

	content := make([]map[string]interface{}, 0, len(textParts)+len(imageParts))
	content = append(content, textParts...)
	content = append(content, imageParts...)
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
	var reasoningParts []string

	for _, block := range msg.ContentBlocks {
		switch block.Type {
		case "text":
			textParts = append(textParts, block.Text)
		case "thinking":
			reasoningParts = append(reasoningParts, block.Thinking)
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
	if len(reasoningParts) > 0 {
		m["reasoning_content"] = strings.Join(reasoningParts, "")
	}
	messages = append(messages, m)
	return messages
}

// openaiBuildTools parses raw tool JSON into OpenAI-format tool definitions.
// It handles two input formats:
//   - OpenAI format: {"type":"function","function":{"name":"...","parameters":{}}}
//   - Anthropic format: {"name":"...","input_schema":{...},"description":"..."}
func openaiBuildTools(toolsRaw []json.RawMessage, strict bool) []map[string]interface{} {
	var tools []map[string]interface{}
	for i, toolJSON := range toolsRaw {
		// Detect OpenAI format: has a "function" key with a map value.
		var probe map[string]json.RawMessage
		if err := json.Unmarshal(toolJSON, &probe); err != nil {
			log.Printf("[openaiBuildTools] tool[%d] unmarshal failed: %v (raw: %s)", i, err, string(toolJSON[:min(len(toolJSON), 100)]))
			continue
		}
		if fnRaw, ok := probe["function"]; ok {
			// OpenAI format — pass through (with optional strict flag).
			var fn map[string]interface{}
			if err := json.Unmarshal(fnRaw, &fn); err != nil {
				log.Printf("[openaiBuildTools] tool[%d] function unmarshal failed: %v", i, err)
				continue
			}
			if strict {
				fn["strict"] = true
			}
			tools = append(tools, map[string]interface{}{
				"type":     "function",
				"function": fn,
			})
			continue
		}

		// Anthropic format: top-level name, input_schema, description.
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
			if err := json.Unmarshal(tool.InputSchema, &inputSchema); err != nil {
				log.Printf("[openaiBuildTools] tool[%d] input_schema unmarshal failed: %v", i, err)
			}
		}
		if inputSchema == nil {
			inputSchema = map[string]interface{}{"type": "object"}
		}
		fn := map[string]interface{}{
			"name":        tool.Name,
			"description": tool.Description,
			"parameters":  inputSchema,
		}
		if strict {
			fn["strict"] = true
		}
		tools = append(tools, map[string]interface{}{
			"type":     "function",
			"function": fn,
		})
	}
	log.Printf("[openaiBuildTools] parsed %d/%d tools: %v", len(tools), len(toolsRaw), func() []string {
		var names []string
		for _, t := range tools {
			if fn, ok := t["function"].(map[string]interface{}); ok {
				if name, ok := fn["name"].(string); ok {
					names = append(names, name)
				}
			}
		}
		return names
	}())
	return tools
}

// openaiAddReasoningContent extracts thinking blocks from original messages
// and injects them as reasoning_content on the corresponding assistant messages
// in the converted output. It walks both slices in order — the output messages
// may have different indices due to system messages and split tool_result blocks,
// so we match by counting assistant-role messages.
func openaiAddReasoningContent(messages []map[string]interface{}, req *Request) []map[string]interface{} {
	// Collect reasoning content from input assistant messages in order.
	var reasoningParts []string
	for _, msg := range req.Messages {
		if msg.Role != "assistant" || len(msg.ContentBlocks) == 0 {
			continue
		}
		var parts []string
		for _, block := range msg.ContentBlocks {
			if block.Type == "thinking" {
				parts = append(parts, block.Thinking)
			}
		}
		reasoningParts = append(reasoningParts, strings.Join(parts, ""))
	}
	if len(reasoningParts) == 0 {
		return messages
	}

	// Walk output messages and assign reasoning to the Nth assistant message.
	assistantIdx := 0
	for i := range messages {
		role, _ := messages[i]["role"].(string)
		if role != "assistant" {
			continue
		}
		if assistantIdx < len(reasoningParts) && reasoningParts[assistantIdx] != "" {
			messages[i]["reasoning_content"] = reasoningParts[assistantIdx]
		}
		assistantIdx++
	}
	return messages
}

// openaiParseSSE reads an SSE stream and emits StreamChunks.
// Handles all known OpenAI-compatible fields across providers:
// content, reasoning_content, tool_calls, finish_reason, usage with all cache variants.
func openaiParseSSE(ctx context.Context, body io.Reader, callback func(StreamChunk) error, finishReasonMap func(string) string, contentArraySupport bool) error {
	type toolCallKey struct {
		choice int
		tool   int
	}
	type toolCallState struct {
		id      string
		name    string
		started bool
	}
	toolCalls := make(map[toolCallKey]*toolCallState)

	// Accumulate usage from usage-only chunks (choices empty) to attach
	// to the real stop/complete event instead of emitting a duplicate stop.
	var pendingUsage *Usage
	var stopped bool

	scanner := bufio.NewScanner(body)
	scanner.Buffer(make([]byte, 0, 32*1024), 256*1024)

	for scanner.Scan() {
		// Check for context cancellation every iteration.
		if ctx.Err() != nil {
			return ctx.Err()
		}
		line := scanner.Text()
		if !strings.HasPrefix(line, "data: ") {
			continue
		}
		data := strings.TrimPrefix(line, "data: ")
		if data == "[DONE]" {
			if err := callback(StreamChunk{Type: "complete", Usage: pendingUsage}); err != nil {
				return err
			}
			return nil
		}
		var chunk struct {
			Choices []struct {
				Index int `json:"index"`
				Delta struct {
					Content          json.RawMessage `json:"content"`
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
			// Usage-only chunk: accumulate usage but don't emit a stop event.
			// The stop will come from finish_reason or [DONE].
			if usage != nil {
				pendingUsage = usage
			}
			continue
		}
		if stopped {
			continue
		}

		choice := chunk.Choices[0]
		choiceIdx := choice.Index
		delta := choice.Delta

		hasReasoning := delta.ReasoningContent != "" || delta.Reasoning != "" || len(delta.ReasoningDetails) > 0
		if len(delta.Content) > 0 && !hasReasoning {
			if contentArraySupport {
				if text := extractContentString(delta.Content); text != "" {
					if err := callback(StreamChunk{Type: "delta", TextDelta: text}); err != nil {
						return err
					}
				}
			} else {
				var s string
				if json.Unmarshal(delta.Content, &s) == nil && s != "" {
					if err := callback(StreamChunk{Type: "delta", TextDelta: s}); err != nil {
						return err
					}
				}
			}
		}
		// Emit thinking from reasoning_details (MiniMax, multi-fragment),
		// reasoning_content (DeepSeek), or reasoning (Groq). Only one source
		// is used per chunk — reasoning_details takes priority since it
		// contains incremental fragments. This avoids emitting the same
		// thinking text multiple times when providers populate several fields.
		if len(delta.ReasoningDetails) > 0 {
			for _, rd := range delta.ReasoningDetails {
				if rd.Text != "" {
					if err := callback(StreamChunk{Type: "delta", Thinking: rd.Text}); err != nil {
						return err
					}
				}
			}
		} else if delta.ReasoningContent != "" {
			if err := callback(StreamChunk{Type: "delta", Thinking: delta.ReasoningContent}); err != nil {
				return err
			}
		} else if delta.Reasoning != "" {
			if err := callback(StreamChunk{Type: "delta", Thinking: delta.Reasoning}); err != nil {
				return err
			}
		}

		for _, tc := range delta.ToolCalls {
			key := toolCallKey{choice: choiceIdx, tool: tc.Index}
			state, ok := toolCalls[key]
			if !ok {
				state = &toolCallState{}
				toolCalls[key] = state
			}
			if tc.ID != "" {
				state.id = tc.ID
			}
			if tc.Function.Name != "" {
				state.name = tc.Function.Name
			}
			// Emit a tool-call-start chunk the first time we see an ID or name,
			// even if there are no arguments yet (handles no-argument tool calls).
			if !state.started && (state.id != "" || state.name != "") {
				state.started = true
				if err := callback(StreamChunk{
					Type:         "delta",
					Index:        tc.Index,
					ToolCallID:   state.id,
					ToolCallName: state.name,
				}); err != nil {
					return err
				}
			}
			// Emit tool call delta whenever we have arguments.
			// OpenAI streams send id+name first, then argument fragments in separate chunks.
			if tc.Function.Arguments != "" {
				if err := callback(StreamChunk{
					Type:         "delta",
					Index:        tc.Index,
					JSONDelta:    tc.Function.Arguments,
					ToolCallID:   state.id,
					ToolCallName: state.name,
				}); err != nil {
					return err
				}
			}
		}

		if choice.FinishReason != nil {
			fr := *choice.FinishReason
			if finishReasonMap != nil {
				fr = finishReasonMap(fr)
			}
			// Normalize context-exceeded finish reasons into an error so
			// the caller gets a single signal path regardless of provider.
			if IsContextExceededFinishReason(fr) {
				err := newAPIError(http.StatusBadRequest, fmt.Sprintf("context window exceeded (finish_reason: %s)", fr))
				err.ContextExceeded = true
				return err
			}
			// Prefer usage from finish_reason chunk; fall back to accumulated usage.
			stopUsage := usage
			if stopUsage == nil {
				stopUsage = pendingUsage
			}
			if err := callback(StreamChunk{Type: "stop", FinishReason: fr, Usage: stopUsage}); err != nil {
				return err
			}
			stopped = true
		}
	}

	return scanner.Err()
}

// IsContextExceededFinishReason returns true for finish reasons that indicate
// the model hit a context window limit rather than a normal stop condition.
func IsContextExceededFinishReason(reason string) bool {
	lower := strings.ToLower(reason)
	return strings.Contains(lower, "context") && strings.Contains(lower, "exceeded") ||
		reason == "model_context_window_exceeded" ||
		reason == "context_length_exceeded" ||
		reason == "content_length_exceeded"
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
	// Take max to avoid double-counting when a provider sends the same
	// cached token value under multiple field names.
	cachedTokens := u.PromptTokensDetails.CachedTokens
	if u.PromptCacheHitTokens > cachedTokens {
		cachedTokens = u.PromptCacheHitTokens
	}
	if u.CachedTokens > cachedTokens {
		cachedTokens = u.CachedTokens
	}
	return &Usage{
		InputTokens:              u.PromptTokens,
		OutputTokens:             u.CompletionTokens,
		CacheReadInputTokens:     cachedTokens,
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
		// Take max to avoid double-counting same value under different field names.
		if details, ok := usageMap["prompt_tokens_details"].(map[string]interface{}); ok {
			if cached, ok := details["cached_tokens"].(float64); ok {
				usage.CacheReadInputTokens = int(cached)
			}
		}
		if hit, ok := usageMap["prompt_cache_hit_tokens"].(float64); ok {
			if int(hit) > usage.CacheReadInputTokens {
				usage.CacheReadInputTokens = int(hit)
			}
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

// extractContentString extracts text from delta.content which may be
// a JSON string ("hello") or a JSON array of [{type:"text",text:"..."}].
func extractContentString(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	// Try plain string first (most common case)
	var s string
	if json.Unmarshal(raw, &s) == nil {
		return s
	}
	// Try array of {type, text} objects
	var parts []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	if json.Unmarshal(raw, &parts) == nil {
		var texts []string
		for _, p := range parts {
			if p.Type == "text" && p.Text != "" {
				texts = append(texts, p.Text)
			}
		}
		return strings.Join(texts, "")
	}
	return ""
}

// fetchModelsHTTP fetches models from any OpenAI-compatible /models endpoint.
// The caller provides the full URL and optional API key with Bearer auth.
func fetchModelsHTTP(ctx context.Context, modelsURL, apiKey string) ([]ModelEntry, error) {
	client := SharedHTTPClient
	req, err := http.NewRequestWithContext(ctx, "GET", modelsURL, nil)
	if err != nil {
		return nil, err
	}
	if apiKey != "" {
		req.Header.Set("Authorization", "Bearer "+apiKey)
	}

	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		errBody, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
		return nil, &ProviderAPIError{
			StatusCode: resp.StatusCode,
			Message:    fmt.Sprintf("/models returned status %d: %s", resp.StatusCode, string(errBody)),
			Retriable:  resp.StatusCode == 429 || resp.StatusCode >= 500,
		}
	}

	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, err
	}

	var result struct {
		Data []struct {
			ID   string `json:"id"`
			Name string `json:"name,omitempty"`
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
	sort.Slice(entries, func(i, j int) bool {
		return entries[i].ID < entries[j].ID
	})
	return entries, nil
}

// ListModels provides default model discovery for openaiCompatHandler-based providers.
// It fetches from BaseURL + "/models" using the standard OpenAI response format.
// Prefers cfg.BaseURL over the built-in default when set (e.g. LM Studio, NVIDIA NIM).
func (h *openaiCompatHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.config.BaseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	url := strings.TrimRight(base, "/") + "/models"
	return fetchModelsHTTP(ctx, url, cfg.APIKey)
}
