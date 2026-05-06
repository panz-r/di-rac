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

// QwenHandler handles Qwen (Alibaba Cloud) API requests.
// Qwen uses an OpenAI-compatible chat completions API with:
//   - Two regions: China (dashscope.aliyuncs.com) and International (dashscope-intl.aliyuncs.com)
//   - R1 format for DeepSeek Reasoner models (merged consecutive same-role messages)
//   - enable_thinking / thinking_budget for Qwen3 reasoning models
//   - reasoning_content in streaming deltas
//   - max_completion_tokens (not max_tokens)
//   - prompt_cache_hit_tokens / prompt_cache_miss_tokens in usage
type QwenHandler struct {
	httpClient *http.Client
	baseURL    string
	apiKey     string
	apiLine    string // "china" or "international"
}

func NewQwenHandler() *QwenHandler {
	return &QwenHandler{
		httpClient: &http.Client{},
		apiLine:    "china",
	}
}

func (h *QwenHandler) getConfig(req *Request) (baseURL, apiKey string) {
	baseURL = h.baseURL
	apiKey = h.apiKey
	if req.Provider.BaseURL != "" {
		baseURL = req.Provider.BaseURL
	}
	if baseURL == "" {
		switch h.apiLine {
		case "international":
			baseURL = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
		default:
			baseURL = "https://dashscope.aliyuncs.com/compatible-mode/v1"
		}
	}
	if req.Provider.APIKey != "" {
		apiKey = req.Provider.APIKey
	}
	return
}

func (h *QwenHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.getConfig(req)
	qwenReq := h.buildRequest(req, false)

	reqBody, err := json.Marshal(qwenReq)
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

func (h *QwenHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.getConfig(req)
	qwenReq := h.buildRequest(req, true)

	reqBody, err := json.Marshal(qwenReq)
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

func (h *QwenHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
}

func (h *QwenHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		switch h.apiLine {
		case "international":
			model = "qwen3-coder-plus"
		default:
			model = "qwen3-235b-a22b"
		}
	}

	isDeepseekReasoner := strings.Contains(model, "deepseek-r1")
	isReasoningFamily := strings.Contains(model, "qwen3") || model == "qwen-plus-latest" || model == "qwen-turbo-latest"
	thinkingOn := req.Thinking != nil && req.Thinking.BudgetTokens > 0

	// Choose message conversion based on model family
	var messages []map[string]interface{}
	openai := &OpenAIHandler{}

	if isDeepseekReasoner || (thinkingOn && isReasoningFamily) {
		// R1 format: merge consecutive same-role messages, include reasoning_content
		messages = h.convertMessagesR1(req)
	} else {
		messages = openai.convertMessages(req)
	}

	result := map[string]interface{}{
		"model":    model,
		"messages": messages,
	}

	// Temperature: 0 normally, omitted when reasoning
	if !isDeepseekReasoner && !(thinkingOn && isReasoningFamily) {
		result["temperature"] = 0
	}

	if stream {
		result["stream"] = true
		result["stream_options"] = map[string]interface{}{"include_usage": true}
	}

	// Qwen uses max_completion_tokens
	if req.MaxTokens > 0 {
		result["max_completion_tokens"] = req.MaxTokens
	}
	if req.TopP > 0 {
		result["top_p"] = req.TopP
	}

	// Thinking params for Qwen3 family
	if isReasoningFamily {
		if thinkingOn {
			result["enable_thinking"] = true
			result["thinking_budget"] = req.Thinking.BudgetTokens
		} else {
			result["enable_thinking"] = false
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

// convertMessagesR1 converts messages with R1 format:
// merges consecutive same-role messages and adds reasoning_content from thinking blocks.
func (h *QwenHandler) convertMessagesR1(req *Request) []map[string]interface{} {
	// Build OpenAI messages first
	openai := &OpenAIHandler{}
	baseMessages := openai.convertMessages(req)

	// Add reasoning_content from thinking blocks
	baseMessages = h.addReasoningContent(baseMessages, req)

	// Merge consecutive same-role messages
	return h.mergeConsecutiveRoles(baseMessages)
}

// addReasoningContent extracts thinking blocks and injects as reasoning_content on assistant messages.
func (h *QwenHandler) addReasoningContent(messages []map[string]interface{}, req *Request) []map[string]interface{} {
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

// mergeConsecutiveRoles merges consecutive messages with the same role by joining their content.
func (h *QwenHandler) mergeConsecutiveRoles(messages []map[string]interface{}) []map[string]interface{} {
	var merged []map[string]interface{}

	for _, msg := range messages {
		if len(merged) == 0 {
			merged = append(merged, msg)
			continue
		}

		last := merged[len(merged)-1]
		lastRole, _ := last["role"].(string)
		curRole, _ := msg["role"].(string)

		if lastRole == curRole {
			// Merge content
			lastContent := contentToString(last["content"])
			curContent := contentToString(msg["content"])
			if lastContent != "" && curContent != "" {
				last["content"] = lastContent + "\n" + curContent
			} else if curContent != "" {
				last["content"] = curContent
			}
		} else {
			merged = append(merged, msg)
		}
	}

	return merged
}

// contentToString extracts a string from content that may be a string or array.
func contentToString(content interface{}) string {
	switch v := content.(type) {
	case string:
		return v
	case []interface{}:
		var parts []string
		for _, item := range v {
			if m, ok := item.(map[string]interface{}); ok {
				if text, _ := m["text"].(string); text != "" {
					parts = append(parts, text)
				}
			}
		}
		return strings.Join(parts, "\n")
	default:
		return ""
	}
}

func (h *QwenHandler) parseSSEStream(body io.Reader, callback func(StreamChunk) error) error {
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
				PromptTokens         int `json:"prompt_tokens"`
				CompletionTokens     int `json:"completion_tokens"`
				PromptCacheHitTokens int `json:"prompt_cache_hit_tokens"`
				PromptCacheMissTokens int `json:"prompt_cache_miss_tokens"`
			} `json:"usage"`
		}

		if err := json.Unmarshal([]byte(data), &chunk); err != nil {
			continue
		}

		if len(chunk.Choices) == 0 {
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

func (h *QwenHandler) convertResponse(resp map[string]interface{}) *SendResult {
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

func (h *QwenHandler) Capabilities() *ProviderInfo {
	defaultModel := "qwen3-235b-a22b"
	if h.apiLine == "international" {
		defaultModel = "qwen3-coder-plus"
	}
	return &ProviderInfo{
		ID:           "qwen",
		DefaultModel: defaultModel,
		Features: ProviderFeatures{
			SupportsThinking:    true,
			SupportsTools:       true,
			SupportsImages:      true,
			SupportsPromptCache: false,
			SupportsStreaming:   true,
		},
		Settings: []ProviderSetting{
			{
				Key:         "temperature",
				Label:       "Temperature",
				Type:        SettingSlider,
				Min:         fPtr(0),
				Max:         fPtr(2),
				Step:        fPtr(0.01),
				Default:     1.0,
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
			{
				Key:         "max_tokens",
				Label:       "Max Tokens",
				Type:        SettingNumber,
				Min:         fPtr(1),
				Group:       "sampling",
				Description: "Maximum tokens in the response (sent as max_completion_tokens).",
			},
			{
				Key:         "enable_thinking",
				Label:       "Enable Thinking",
				Type:        SettingToggle,
				Group:       "reasoning",
				Description: "Enable thinking/reasoning for Qwen3 and R1 models.",
			},
			{
				Key:         "thinking_budget",
				Label:       "Thinking Budget",
				Type:        SettingNumber,
				Min:         fPtr(1),
				Group:       "reasoning",
				Description: "Token budget for thinking (only applies when thinking is enabled).",
			},
		},
	}
}

var _ CapableHandler = (*QwenHandler)(nil)

func (h *QwenHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.baseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	if base == "" {
		base = "https://dashscope.aliyuncs.com/compatible-mode/v1"
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", h.apiKey)
}

var _ ModelLister = (*QwenHandler)(nil)
