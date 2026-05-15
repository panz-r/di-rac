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

// ResponsesAPIConfig configures a provider using the OpenAI Responses API format.
type ResponsesAPIConfig struct {
	BaseURL      string
	DefaultModel string
	// ModifyRequest is called after the standard Responses API request is built.
	ModifyRequest func(req *Request, result map[string]interface{})
	// ModifyHeaders is called after default headers are set.
	ModifyHeaders func(httpReq *http.Request, apiKey string)
	// Capabilities declares this provider's supported settings and features.
	Capabilities *ProviderInfo
}

// responsesAPIHandler implements Handler for the OpenAI Responses API (/responses).
type responsesAPIHandler struct {
	httpClient *http.Client
	config     ResponsesAPIConfig
}

func newResponsesAPIHandler(config ResponsesAPIConfig) *responsesAPIHandler {
	return &responsesAPIHandler{
		httpClient: SharedHTTPClient,
		config:     config,
	}
}

func (h *responsesAPIHandler) Capabilities() *ProviderInfo {
	return h.config.Capabilities
}

// --- Send ---

func (h *responsesAPIHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.resolveConfig(req)
	payload := h.buildRequest(req, false)

	body, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("marshal responses request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", strings.TrimRight(baseURL, "/")+"/responses", bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}
	h.setHeaders(httpReq, apiKey)

	resp, err := h.httpClient.Do(httpReq)
	if err != nil {
		return nil, &ProviderAPIError{StatusCode: 0, Message: err.Error(), Retriable: false}
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, &ProviderAPIError{StatusCode: resp.StatusCode, Message: fmt.Sprintf("read response: %v", err), Retriable: false}
	}

	if resp.StatusCode != http.StatusOK {
		msg := string(respBody)
		retriable := resp.StatusCode == 429 || resp.StatusCode >= 500
		return nil, &ProviderAPIError{StatusCode: resp.StatusCode, Message: msg, Retriable: retriable}
	}

	return h.parseResponse(respBody)
}

// --- Stream ---

func (h *responsesAPIHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.resolveConfig(req)
	payload := h.buildRequest(req, true)

	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal responses request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", strings.TrimRight(baseURL, "/")+"/responses", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("create request: %w", err)
	}
	h.setHeaders(httpReq, apiKey)
	httpReq.Header.Set("Accept", "text/event-stream")
	httpReq.Header.Set("Cache-Control", "no-cache")

	resp, err := h.httpClient.Do(httpReq)
	if err != nil {
		return &ProviderAPIError{StatusCode: 0, Message: err.Error(), Retriable: false}
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		respBody, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
		msg := string(respBody)
		retriable := resp.StatusCode == 429 || resp.StatusCode >= 500
		return &ProviderAPIError{StatusCode: resp.StatusCode, Message: msg, Retriable: retriable}
	}

	return parseResponsesSSE(ctx, &contextReader{ctx: ctx, r: resp.Body}, callback)
}

// --- Config helpers ---

func (h *responsesAPIHandler) resolveConfig(req *Request) (baseURL, apiKey string) {
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

func (h *responsesAPIHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
	if h.config.ModifyHeaders != nil {
		h.config.ModifyHeaders(httpReq, apiKey)
	}
}

// --- Request building ---

func (h *responsesAPIHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = h.config.DefaultModel
	}

	result := map[string]interface{}{
		"model": model,
		"store": false,
	}

	// Instructions (replaces system message)
	if req.System != "" {
		result["instructions"] = req.System
	}

	// Input (converts messages to Responses API input items)
	result["input"] = responsesConvertMessages(req)

	if stream {
		result["stream"] = true
	}

	// Sampling parameters
	if req.Temperature != 0 {
		result["temperature"] = req.Temperature
	}
	if req.TopP > 0 {
		result["top_p"] = req.TopP
	}
	if req.MaxTokens > 0 {
		result["max_output_tokens"] = req.MaxTokens
	}

	// Tools
	if len(req.Tools) > 0 {
		tools := responsesBuildTools(req.Tools)
		if len(tools) > 0 {
			result["tools"] = tools
			result["tool_choice"] = "auto"
		}
	}

	// Thinking / reasoning
	if req.Thinking != nil {
		effort := "high"
		if req.Thinking.ReasoningEffort != "" {
			effort = req.Thinking.ReasoningEffort
		}
		result["reasoning"] = map[string]interface{}{
			"effort":  effort,
			"summary": "auto",
		}
		result["include"] = []string{"reasoning.encrypted_content"}
	}

	// Provider-specific modifications
	if h.config.ModifyRequest != nil {
		h.config.ModifyRequest(req, result)
	}

	return result
}

// responsesConvertMessages converts our messages to Responses API input items.
func responsesConvertMessages(req *Request) []interface{} {
	var items []interface{}

	for _, msg := range req.Messages {
		// Tool results → function_call_output items
		if msg.ToolResult != nil {
			items = append(items, map[string]interface{}{
				"type":    "function_call_output",
				"call_id": msg.ToolResult.ToolUseID,
				"output":  msg.ToolResult.Content,
			})
			continue
		}

		// Tool calls from assistant → function_call items
		if len(msg.ToolCalls) > 0 {
			for _, tc := range msg.ToolCalls {
				items = append(items, map[string]interface{}{
					"type":      "function_call",
					"call_id":   tc.ID,
					"name":      tc.Function.Name,
					"arguments": tc.Function.Arguments,
				})
			}
			// If there's also text content, add a message item
			if msg.Content != "" {
				items = append(items, map[string]interface{}{
					"type": "message",
					"role": "assistant",
					"content": []interface{}{
						map[string]interface{}{"type": "output_text", "text": msg.Content},
					},
				})
			}
			continue
		}

		// Content blocks
		if len(msg.ContentBlocks) > 0 {
			items = appendContentBlockItems(items, msg)
			continue
		}

		// Thinking
		if msg.Thinking != "" {
			items = append(items, map[string]interface{}{
				"type": "reasoning",
				"summary": []interface{}{
					map[string]interface{}{"type": "summary_text", "text": msg.Thinking},
				},
			})
			continue
		}

		// Plain text message
		if msg.Content != "" {
			contentType := "input_text"
			if msg.Role == "assistant" {
				contentType = "output_text"
			}
			items = append(items, map[string]interface{}{
				"type": "message",
				"role":  msg.Role,
				"content": []interface{}{
					map[string]interface{}{"type": contentType, "text": msg.Content},
				},
			})
		}
	}

	return items
}

func appendContentBlockItems(items []interface{}, msg Message) []interface{} {
	var contentParts []interface{}

	for _, block := range msg.ContentBlocks {
		switch block.Type {
		case "text":
			ct := "input_text"
			if msg.Role == "assistant" {
				ct = "output_text"
			}
			contentParts = append(contentParts, map[string]interface{}{
				"type": ct,
				"text": block.Text,
			})
		case "image":
			if block.ImageSource != nil {
				imgURL := ""
				if block.ImageSource.Data != "" {
					imgURL = "data:" + block.ImageSource.MimeType + ";base64," + block.ImageSource.Data
				} else if block.ImageSource.URL != "" {
					imgURL = block.ImageSource.URL
				}
				if imgURL != "" {
					contentParts = append(contentParts, map[string]interface{}{
						"type":      "input_image",
						"image_url": imgURL,
						"detail":    "auto",
					})
				}
			}
		case "tool_use":
			if block.ToolUse != nil {
				items = append(items, map[string]interface{}{
					"type":      "function_call",
					"call_id":   block.ToolUse.ID,
					"name":      block.ToolUse.Function.Name,
					"arguments": block.ToolUse.Function.Arguments,
				})
			}
		case "tool_result":
			if block.ToolResult != nil {
				items = append(items, map[string]interface{}{
					"type":    "function_call_output",
					"call_id": block.ToolResult.ToolUseID,
					"output":  block.ToolResult.Content,
				})
			}
		case "thinking":
			items = append(items, map[string]interface{}{
				"type": "reasoning",
				"summary": []interface{}{
					map[string]interface{}{"type": "summary_text", "text": block.Thinking},
				},
			})
		}
	}

	if len(contentParts) > 0 {
		items = append(items, map[string]interface{}{
			"type":    "message",
			"role":    msg.Role,
			"content": contentParts,
		})
	}

	return items
}

// responsesBuildTools converts tool definitions to Responses API format.
// Responses API uses internally-tagged format: {"type": "function", "name": "...", ...}
func responsesBuildTools(tools []json.RawMessage) []interface{} {
	var result []interface{}
	for _, raw := range tools {
		var t map[string]interface{}
		if err := json.Unmarshal(raw, &t); err != nil {
			continue
		}

		// Anthropic-style tool with top-level name: convert input_schema to parameters
		if name, ok := t["name"].(string); ok {
			tool := map[string]interface{}{
				"type": "function",
				"name": name,
			}
			if desc, ok := t["description"].(string); ok {
				tool["description"] = desc
			}
			if schema, ok := t["input_schema"]; ok {
				tool["parameters"] = schema
			} else if params, ok := t["parameters"]; ok {
				tool["parameters"] = params
			}
			if strict, ok := t["strict"].(bool); ok {
				tool["strict"] = strict
			}
			result = append(result, tool)
			continue
		}

		// Chat Completions format: {"type":"function","function":{"name":"...","parameters":{...}}}
		if fn, ok := t["function"].(map[string]interface{}); ok {
			tool := map[string]interface{}{
				"type": "function",
			}
			if name, ok := fn["name"].(string); ok {
				tool["name"] = name
			}
			if desc, ok := fn["description"].(string); ok {
				tool["description"] = desc
			}
			if params, ok := fn["parameters"]; ok {
				tool["parameters"] = params
			}
			if strict, ok := fn["strict"].(bool); ok {
				tool["strict"] = strict
			}
			result = append(result, tool)
		}
		// Skip tools we can't parse
	}
	return result
}

// --- Response parsing ---

func (h *responsesAPIHandler) parseResponse(body []byte) (*SendResult, error) {
	var raw map[string]interface{}
	if err := json.Unmarshal(body, &raw); err != nil {
		return nil, fmt.Errorf("parse responses API response: %w", err)
	}

	output, _ := raw["output"].([]interface{})
	var contentBlocks []ContentBlock
	var stopReason string

	for _, item := range output {
		itemMap, ok := item.(map[string]interface{})
		if !ok {
			continue
		}
		itemType, _ := itemMap["type"].(string)

		switch itemType {
		case "message":
			msgContent, _ := itemMap["content"].([]interface{})
			for _, c := range msgContent {
				cMap, ok := c.(map[string]interface{})
				if !ok {
					continue
				}
				cType, _ := cMap["type"].(string)
				switch cType {
				case "output_text":
					text, _ := cMap["text"].(string)
					contentBlocks = append(contentBlocks, ContentBlock{
						Type: "text",
						Text: text,
					})
				}
			}
		case "function_call":
			callID, _ := itemMap["call_id"].(string)
			name, _ := itemMap["name"].(string)
			arguments, _ := itemMap["arguments"].(string)
			contentBlocks = append(contentBlocks, ContentBlock{
				Type: "tool_use",
				ToolUse: &ToolUseBlock{
					ID:   callID,
					Type: "function",
					Function: struct {
						Name      string `json:"name"`
						Arguments string `json:"arguments"`
					}{Name: name, Arguments: arguments},
				},
			})
		case "reasoning":
			summary, _ := itemMap["summary"].([]interface{})
			for _, s := range summary {
				sMap, ok := s.(map[string]interface{})
				if !ok {
					continue
				}
				if sType, _ := sMap["type"].(string); sType == "summary_text" {
					text, _ := sMap["text"].(string)
					if text != "" {
						contentBlocks = append(contentBlocks, ContentBlock{
							Type:     "thinking",
							Thinking: text,
						})
					}
				}
			}
		}

		// Check status for stop reason
		if status, _ := itemMap["status"].(string); status == "completed" && stopReason == "" {
			stopReason = "stop"
		}
	}

	if stopReason == "" {
		stopReason = "stop"
	}

	// Parse usage
	var usage *Usage
	if u, ok := raw["usage"].(map[string]interface{}); ok {
		usage = &Usage{}
		if v, ok := u["input_tokens"].(float64); ok {
			usage.InputTokens = int(v)
		}
		if v, ok := u["output_tokens"].(float64); ok {
			usage.OutputTokens = int(v)
		}
		if v, ok := u["total_tokens"].(float64); ok {
			usage.TotalTokens = int(v)
		}
		// Nested details
		if details, ok := u["input_tokens_details"].(map[string]interface{}); ok {
			if v, ok := details["cached_tokens"].(float64); ok {
				usage.CacheReadInputTokens = int(v)
			}
		}
		if details, ok := u["output_tokens_details"].(map[string]interface{}); ok {
			if v, ok := details["reasoning_tokens"].(float64); ok {
				usage.ReasoningTokens = int(v)
			}
		}
		usage.TotalTokens = usage.InputTokens + usage.OutputTokens
	}

	model, _ := raw["model"].(string)

	return &SendResult{
		Content:    contentBlocks,
		StopReason: stopReason,
		Usage:      usage,
		Model:      model,
		Raw:        body,
	}, nil
}

// --- SSE streaming ---

func parseResponsesSSE(ctx context.Context, body io.Reader, callback func(StreamChunk) error) error {
	scanner := bufio.NewScanner(body)
	scanner.Buffer(make([]byte, 0, 32*1024), 256*1024)

	var eventType string
	var dataBuf strings.Builder
	var processErr error

	processEvent := func() {
		if processErr != nil {
			return
		}
		data := dataBuf.String()
		dataBuf.Reset()
		if data == "" {
			return
		}

		var evt map[string]interface{}
		if err := json.Unmarshal([]byte(data), &evt); err != nil {
			log.Printf("responses SSE: parse error: %v", err)
			return
		}

		switch eventType {
		case "response.output_text.delta":
			delta, _ := evt["delta"].(string)
			if delta != "" {
				if err := callback(StreamChunk{Type: "delta", TextDelta: delta}); err != nil {
					processErr = err
					return
				}
			}

		case "response.function_call_arguments.delta":
			delta, _ := evt["delta"].(string)
			idx := intFloat(evt["output_index"])
			callID, _ := evt["call_id"].(string)
			if delta != "" || callID != "" {
				if err := callback(StreamChunk{
					Type:       "delta",
					JSONDelta:  delta,
					Index:      idx,
					ToolCallID: callID,
				}); err != nil {
					processErr = err
					return
				}
			}

		case "response.output_item.added":
			item, _ := evt["item"].(map[string]interface{})
			if item != nil {
				if itemType, _ := item["type"].(string); itemType == "function_call" {
					callID, _ := item["call_id"].(string)
					name, _ := item["name"].(string)
					idx := intFloat(evt["output_index"])
					if err := callback(StreamChunk{
						Type:         "delta",
						Index:        idx,
						ToolCallID:   callID,
						ToolCallName: name,
					}); err != nil {
						processErr = err
						return
					}
				}
			}

		case "response.reasoning_summary_text.delta":
			delta, _ := evt["delta"].(string)
			if delta != "" {
				if err := callback(StreamChunk{Type: "delta", Thinking: delta}); err != nil {
					processErr = err
					return
				}
			}

		case "response.reasoning_text.delta":
			delta, _ := evt["delta"].(string)
			if delta != "" {
				if err := callback(StreamChunk{Type: "delta", Thinking: delta}); err != nil {
					processErr = err
					return
				}
			}

		case "response.completed":
			var usage *Usage
			if resp, ok := evt["response"].(map[string]interface{}); ok {
				usage = parseResponsesUsage(resp)
			}
			if err := callback(StreamChunk{Type: "complete", Usage: usage}); err != nil {
				processErr = err
				return
			}

		case "response.failed":
			statusCode := 500
			if resp, ok := evt["response"].(map[string]interface{}); ok {
				var code, message string
				if lastErr, ok := resp["last_error"].(map[string]interface{}); ok {
					code, _ = lastErr["code"].(string)
					message, _ = lastErr["message"].(string)
				}
				if code == "rate_limit_exceeded" {
					statusCode = 429
				}
				processErr = &ProviderAPIError{StatusCode: statusCode, Message: fmt.Sprintf("%s: %s", code, message), Retriable: statusCode == 429 || statusCode >= 500}
				return
			}
			processErr = &ProviderAPIError{StatusCode: 500, Message: "responses API: response failed", Retriable: true}
			return

		case "error":
			msg, _ := evt["message"].(string)
			if msg == "" {
				msg = data
			}
			processErr = &ProviderAPIError{StatusCode: 500, Message: msg, Retriable: true}
			return
		}
	}

	for scanner.Scan() {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		line := scanner.Text()

		// Empty line (or whitespace-only) = event boundary
		if strings.TrimSpace(line) == "" {
			processEvent()
			if processErr != nil {
				return processErr
			}
			eventType = ""
			continue
		}

		// Parse event type
		if strings.HasPrefix(line, "event: ") {
			eventType = strings.TrimPrefix(line, "event: ")
			continue
		}

		// Accumulate data lines
		if strings.HasPrefix(line, "data: ") {
			if dataBuf.Len() > 0 {
				dataBuf.WriteByte('\n')
			}
			dataBuf.WriteString(strings.TrimPrefix(line, "data: "))
		}
	}

	// Flush any remaining data
	processEvent()
	if processErr != nil {
		return processErr
	}

	if err := scanner.Err(); err != nil {
		return fmt.Errorf("responses SSE scanner error: %w", err)
	}

	return nil
}

func parseResponsesUsage(resp map[string]interface{}) *Usage {
	usage := &Usage{}
	u, _ := resp["usage"].(map[string]interface{})
	if u == nil {
		return usage
	}
	if v, ok := u["input_tokens"].(float64); ok {
		usage.InputTokens = int(v)
	}
	if v, ok := u["output_tokens"].(float64); ok {
		usage.OutputTokens = int(v)
	}
	if details, ok := u["input_tokens_details"].(map[string]interface{}); ok {
		if v, ok := details["cached_tokens"].(float64); ok {
			usage.CacheReadInputTokens = int(v)
		}
	}
	if details, ok := u["output_tokens_details"].(map[string]interface{}); ok {
		if v, ok := details["reasoning_tokens"].(float64); ok {
			usage.ReasoningTokens = int(v)
		}
	}
	usage.TotalTokens = usage.InputTokens + usage.OutputTokens
	return usage
}

// intFloat extracts an int from a float64 JSON value.
func intFloat(v interface{}) int {
	if f, ok := v.(float64); ok {
		return int(f)
	}
	return 0
}
