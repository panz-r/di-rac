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

// FireworksHandler handles Fireworks AI API requests.
// Fireworks uses an OpenAI-compatible chat completions API with:
//   - Base URL: https://api.fireworks.ai/inference/v1
//   - R1 format for models marked isR1FormatRequired (addReasoningContent)
//   - <think/> tag detection in content for reasoning extraction
//   - reasoning_content field in streaming deltas
//   - prompt_cache_hit_tokens / prompt_cache_miss_tokens in usage
type FireworksHandler struct {
	httpClient *http.Client
	baseURL    string
	apiKey     string
}

func NewFireworksHandler() *FireworksHandler {
	return &FireworksHandler{
		httpClient: &http.Client{},
		baseURL:    "https://api.fireworks.ai/inference/v1",
	}
}

func (h *FireworksHandler) getConfig(req *Request) (baseURL, apiKey string) {
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

func (h *FireworksHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.getConfig(req)
	fwReq := h.buildRequest(req, false)

	reqBody, err := json.Marshal(fwReq)
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

func (h *FireworksHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.getConfig(req)
	fwReq := h.buildRequest(req, true)

	reqBody, err := json.Marshal(fwReq)
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

func (h *FireworksHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
}

func (h *FireworksHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	// Reuse OpenAI message conversion, with optional R1 addReasoningContent
	openai := &OpenAIHandler{}
	messages := openai.convertMessages(req)

	// Add reasoning_content from thinking blocks for R1-format models
	messages = h.addReasoningContent(messages, req)

	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = "accounts/fireworks/models/kimi-k2p6"
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

	return result
}

// addReasoningContent extracts thinking blocks from original messages
// and adds them as reasoning_content on assistant messages.
func (h *FireworksHandler) addReasoningContent(messages []map[string]interface{}, req *Request) []map[string]interface{} {
	reasoningByIndex := map[int]string{}

	msgIdx := 0
	if req.System != "" {
		msgIdx++
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

func (h *FireworksHandler) parseSSEStream(body io.Reader, callback func(StreamChunk) error) error {
	scanner := bufio.NewScanner(body)
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	// Track whether we're inside <think/> tags for reasoning extraction
	var reasoningAccum *strings.Builder

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

		content := delta.Content
		hasReasoningField := delta.ReasoningContent != ""

		// Track <think/> tag reasoning state
		if reasoningAccum != nil || strings.Contains(content, "<think") {
			if reasoningAccum == nil {
				reasoningAccum = &strings.Builder{}
			}
			reasoningAccum.WriteString(content)

			if hasReasoningField {
				callback(StreamChunk{Type: "delta", Thinking: delta.ReasoningContent})
			} else {
				callback(StreamChunk{Type: "delta", Thinking: content})
			}

			// End of think block
			if strings.Contains(reasoningAccum.String(), "</think") {
				reasoningAccum = nil
			}
		} else if content != "" {
			callback(StreamChunk{Type: "delta", TextDelta: content})
		} else if hasReasoningField {
			callback(StreamChunk{Type: "delta", Thinking: delta.ReasoningContent})
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

func (h *FireworksHandler) convertResponse(resp map[string]interface{}) *SendResult {
	var content []ContentBlock

	if choices, ok := resp["choices"].([]interface{}); ok && len(choices) > 0 {
		if choice, ok := choices[0].(map[string]interface{}); ok {
			if msg, ok := choice["message"].(map[string]interface{}); ok {
				if text, ok := msg["content"].(string); ok && text != "" {
					content = append(content, ContentBlock{Type: "text", Text: text})
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

func (h *FireworksHandler) Capabilities() *ProviderInfo {
	return &ProviderInfo{
		ID:           "fireworks",
		DefaultModel: "accounts/fireworks/models/kimi-k2p6",
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
				Description: "Maximum tokens in the response.",
			},
			{
				Key:        "presence_penalty",
				Label:      "Presence Penalty",
				Type:       SettingSlider,
				Min:        fPtr(-2),
				Max:        fPtr(2),
				Step:       fPtr(0.1),
				Group:      "sampling",
				ValidRange: "-2 – 2",
			},
			{
				Key:        "frequency_penalty",
				Label:      "Frequency Penalty",
				Type:       SettingSlider,
				Min:        fPtr(-2),
				Max:        fPtr(2),
				Step:       fPtr(0.1),
				Group:      "sampling",
				ValidRange: "-2 – 2",
			},
		},
	}
}

var _ CapableHandler = (*FireworksHandler)(nil)

func (h *FireworksHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.baseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", h.apiKey)
}

var _ ModelLister = (*FireworksHandler)(nil)
