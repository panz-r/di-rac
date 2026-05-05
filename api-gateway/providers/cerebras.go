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

// CerebrasHandler handles Cerebras API requests.
// Cerebras uses a text-only message format (no images/tool_calls in history).
// Qwen reasoning models emit <think/> tags that are tracked for reasoning extraction.
type CerebrasHandler struct {
	httpClient *http.Client
	baseURL    string
	apiKey     string
}

func NewCerebrasHandler() *CerebrasHandler {
	return &CerebrasHandler{
		httpClient: &http.Client{},
		baseURL:    "https://api.cerebras.ai/v1",
	}
}

func (h *CerebrasHandler) getConfig(req *Request) (baseURL, apiKey string) {
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

func (h *CerebrasHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.getConfig(req)
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
	return openaiConvertResponse(raw, nil), nil
}

func (h *CerebrasHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.getConfig(req)
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

	return h.parseSSEStream(resp.Body, callback, req)
}

func (h *CerebrasHandler) setHeaders(httpReq *http.Request, apiKey string) {
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("X-Cerebras-3rd-Party-Integration", "dirac")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
}

func (h *CerebrasHandler) buildRequest(req *Request, stream bool) map[string]interface{} {
	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = "zai-glm-4.7"
	}

	// Cerebras uses text-only messages (no images, no tool_calls)
	messages := h.convertTextMessages(req)

	result := map[string]interface{}{
		"model":       model,
		"messages":    messages,
		"temperature": 0,
		"max_tokens":  16384,
	}

	if stream {
		result["stream"] = true
	}

	return result
}

// convertTextMessages produces simple {role, content} string messages.
// Images are replaced with "[Image content not supported in Cerebras]".
// Thinking tags are stripped from assistant messages for reasoning models.
func (h *CerebrasHandler) convertTextMessages(req *Request) []map[string]interface{} {
	var messages []map[string]interface{}
	isReasoning := strings.Contains(strings.ToLower(req.Provider.Model), "qwen")

	if req.System != "" {
		messages = append(messages, map[string]interface{}{
			"role":    "system",
			"content": req.System,
		})
	}

	for _, msg := range req.Messages {
		var content string
		if len(msg.ContentBlocks) > 0 {
			var parts []string
			for _, block := range msg.ContentBlocks {
				switch block.Type {
				case "text":
					parts = append(parts, block.Text)
				case "image":
					parts = append(parts, "[Image content not supported in Cerebras]")
				}
			}
			content = strings.Join(parts, "\n")
		} else {
			content = msg.Content
		}

		// Strip <think/> tags from assistant messages for reasoning models
		if msg.Role == "assistant" && isReasoning {
			content = stripThinkTags(content)
		}

		messages = append(messages, map[string]interface{}{
			"role":    msg.Role,
			"content": content,
		})
	}

	return messages
}

func (h *CerebrasHandler) parseSSEStream(body io.Reader, callback func(StreamChunk) error, req *Request) error {
	model := strings.ToLower(req.Provider.Model)
	if req.ModelOverride != "" {
		model = strings.ToLower(req.ModelOverride)
	}
	isReasoning := strings.Contains(model, "qwen")

	scanner := bufio.NewScanner(body)
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	var reasoning *strings.Builder

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
					Content string `json:"content"`
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
				callback(StreamChunk{Type: "stop", Usage: &Usage{
					InputTokens:  chunk.Usage.PromptTokens,
					OutputTokens: chunk.Usage.CompletionTokens,
				}})
			}
			continue
		}

		choice := chunk.Choices[0]
		content := choice.Delta.Content

		if isReasoning && content != "" {
			// Track <think/> blocks for reasoning models
			if reasoning != nil || strings.Contains(content, "<think") {
				if reasoning == nil {
					reasoning = &strings.Builder{}
				}
				reasoning.WriteString(content)
				clean := strings.ReplaceAll(content, "<think", "")
				clean = strings.ReplaceAll(clean, "</think", "")
				clean = strings.TrimSpace(clean)
				if clean != "" {
					callback(StreamChunk{Type: "delta", Thinking: clean})
				}
				if strings.Contains(reasoning.String(), "</think") {
					reasoning = nil
				}
			} else {
				callback(StreamChunk{Type: "delta", TextDelta: content})
			}
		} else if content != "" {
			callback(StreamChunk{Type: "delta", TextDelta: content})
		}

		if choice.FinishReason != nil {
			callback(StreamChunk{Type: "stop", FinishReason: *choice.FinishReason, Usage: &Usage{}})
		}
	}

	return nil
}

// stripThinkTags removes <think...</think...> blocks from content.
func stripThinkTags(content string) string {
	for {
		start := strings.Index(content, "<think")
		if start == -1 {
			break
		}
		end := strings.Index(content[start:], "</think")
		if end == -1 {
			break
		}
		content = content[:start] + content[start+end+len("</think"):]
	}
	return strings.TrimSpace(content)
}

var _ Handler = (*CerebrasHandler)(nil)

func (h *CerebrasHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	base := h.baseURL
	if cfg.BaseURL != "" {
		base = cfg.BaseURL
	}
	return fetchModelsHTTP(ctx, strings.TrimRight(base, "/")+"/models", h.apiKey)
}

var _ ModelLister = (*CerebrasHandler)(nil)
