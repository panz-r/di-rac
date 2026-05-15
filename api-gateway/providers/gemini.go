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

// GeminiHandler handles Gemini API requests via direct HTTP.
type GeminiHandler struct{}

func NewGeminiHandler() *GeminiHandler {
	return &GeminiHandler{}
}

func NewGeminiHandlerWithKey(apiKey string) (*GeminiHandler, error) {
	return &GeminiHandler{}, nil
}

func (h *GeminiHandler) resolveConfig(req *Request) (baseURL, apiKey string) {
	baseURL = "https://generativelanguage.googleapis.com"
	if req.Provider.BaseURL != "" {
		baseURL = req.Provider.BaseURL
	}
	apiKey = req.Provider.APIKey
	return
}

func (h *GeminiHandler) resolveModel(req *Request) string {
	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = "gemini-3-flash-preview"
	}
	return model
}

// buildRequestPayload builds the Gemini REST API request body.
func (h *GeminiHandler) buildRequestPayload(req *Request) (map[string]interface{}, error) {
	// Pre-scan to build tool_use_id → function name map
	toolUseIDToName := map[string]string{}
	for _, msg := range req.Messages {
		for _, block := range msg.ContentBlocks {
			if block.Type == "tool_use" && block.ToolUse != nil {
				toolUseIDToName[block.ToolUse.ID] = block.ToolUse.Function.Name
			}
		}
	}

	var contents []map[string]interface{}
	var lastUserParts []map[string]interface{}

	for i, msg := range req.Messages {
		role := "user"
		if msg.Role == "assistant" {
			role = "model"
		}

		var parts []map[string]interface{}
		if len(msg.ContentBlocks) > 0 {
			parts = geminiConvertContentBlocks(msg.ContentBlocks, toolUseIDToName)
		} else {
			// Legacy fallback
			if msg.Content != "" {
				parts = append(parts, map[string]interface{}{"text": msg.Content})
			}
			if msg.Thinking != "" {
				parts = append(parts, map[string]interface{}{"text": msg.Thinking})
			}
			if msg.ToolResult != nil {
				name := msg.ToolResult.ToolUseID
				if n, ok := toolUseIDToName[msg.ToolResult.ToolUseID]; ok {
					name = n
				}
				parts = append(parts, map[string]interface{}{
					"functionResponse": map[string]interface{}{
						"name":     name,
						"response": map[string]interface{}{"result": msg.ToolResult.Content},
					},
				})
			}
		}

		if len(parts) == 0 {
			continue
		}

		// Last user message gets sent separately, rest goes into contents as history
		isLast := i == len(req.Messages)-1 && msg.Role == "user"
		if isLast {
			lastUserParts = parts
		} else {
			contents = append(contents, map[string]interface{}{
				"role":  role,
				"parts": parts,
			})
		}
	}

	// Build the request payload
	payload := map[string]interface{}{}

	// System instruction
	if req.System != "" {
		payload["systemInstruction"] = map[string]interface{}{
			"parts": []map[string]interface{}{{"text": req.System}},
		}
	}

	// Generation config
	genConfig := map[string]interface{}{}
	if req.Temperature > 0 {
		genConfig["temperature"] = req.Temperature
	}
	if req.TopP > 0 {
		genConfig["topP"] = req.TopP
	}
	if req.MaxTokens > 0 {
		genConfig["maxOutputTokens"] = req.MaxTokens
	}
	if len(req.Stop) > 0 {
		genConfig["stopSequences"] = req.Stop
	}
	if len(genConfig) > 0 {
		payload["generationConfig"] = genConfig
	}

	// Tools
	if len(req.Tools) > 0 {
		var decls []map[string]interface{}
		for _, toolJSON := range req.Tools {
			var tool struct {
				Name        string          `json:"name"`
				Description string          `json:"description"`
				InputSchema json.RawMessage `json:"input_schema"`
				Parameters  json.RawMessage `json:"parameters"`
			}
			if err := json.Unmarshal(toolJSON, &tool); err != nil {
				continue
			}
			fd := map[string]interface{}{
				"name": tool.Name,
			}
			if tool.Description != "" {
				fd["description"] = tool.Description
			}
			// Google format: parameters is already a Gemini-compatible schema
			if len(tool.Parameters) > 0 {
				var schema interface{}
				if json.Unmarshal(tool.Parameters, &schema) == nil {
					fd["parameters"] = schema
				}
			} else if len(tool.InputSchema) > 0 {
				// Anthropic format: convert JSON Schema to Gemini schema
				fd["parameters"] = jsonSchemaToGeminiSchema(tool.InputSchema)
			}
			decls = append(decls, fd)
		}
		if len(decls) > 0 {
			payload["tools"] = []map[string]interface{}{
				{"functionDeclarations": decls},
			}
		}
	}

	// Contents: history + last user message
	if len(lastUserParts) == 0 {
		return nil, fmt.Errorf("gemini: conversation must end with a user message")
	}
	contents = append(contents, map[string]interface{}{
		"role":  "user",
		"parts": lastUserParts,
	})
	payload["contents"] = contents

	return payload, nil
}

// geminiConvertContentBlocks converts content blocks to Gemini part objects.
func geminiConvertContentBlocks(blocks []ContentBlock, toolUseIDToName map[string]string) []map[string]interface{} {
	var parts []map[string]interface{}
	for _, block := range blocks {
		switch block.Type {
		case "text":
			parts = append(parts, map[string]interface{}{"text": block.Text})
		case "thinking":
			parts = append(parts, map[string]interface{}{"text": block.Thinking})
		case "image":
			if block.ImageSource != nil && block.ImageSource.Data != "" {
				parts = append(parts, map[string]interface{}{
					"inlineData": map[string]interface{}{
						"mimeType": block.ImageSource.MimeType,
						"data":     block.ImageSource.Data,
					},
				})
			}
		case "tool_use":
			if block.ToolUse != nil {
				var args map[string]interface{}
				if err := json.Unmarshal([]byte(block.ToolUse.Function.Arguments), &args); err != nil {
					args = map[string]interface{}{}
				}
				parts = append(parts, map[string]interface{}{
					"functionCall": map[string]interface{}{
						"name": block.ToolUse.Function.Name,
						"args": args,
					},
				})
			}
		case "tool_result":
			if block.ToolResult != nil {
				name := block.ToolResult.ToolUseID
				if n, ok := toolUseIDToName[block.ToolResult.ToolUseID]; ok {
					name = n
				}
				parts = append(parts, map[string]interface{}{
					"functionResponse": map[string]interface{}{
						"name":     name,
						"response": map[string]interface{}{"result": block.ToolResult.Content},
					},
				})
			}
		}
	}
	return parts
}

// jsonSchemaToGeminiSchema converts a JSON Schema object to a Gemini-format schema map.
func jsonSchemaToGeminiSchema(schemaJSON json.RawMessage) map[string]interface{} {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(schemaJSON, &raw); err != nil {
		return map[string]interface{}{"type": "OBJECT"}
	}

	schema := map[string]interface{}{"type": "OBJECT"}

	if t, ok := raw["type"]; ok {
		var typeStr string
		if json.Unmarshal(t, &typeStr) == nil {
			schema["type"] = mapJSONTypeToGemini(typeStr)
		}
	}

	if d, ok := raw["description"]; ok {
		var desc string
		if json.Unmarshal(d, &desc) == nil {
			schema["description"] = desc
		}
	}

	if props, ok := raw["properties"]; ok {
		var propsMap map[string]json.RawMessage
		if json.Unmarshal(props, &propsMap) == nil {
			properties := make(map[string]interface{})
			for name, propJSON := range propsMap {
				properties[name] = jsonSchemaToGeminiSchema(propJSON)
			}
			schema["properties"] = properties
		}
	}

	if req, ok := raw["required"]; ok {
		var required []string
		if json.Unmarshal(req, &required) == nil {
			schema["required"] = required
		}
	}

	return schema
}

func mapJSONTypeToGemini(t string) string {
	switch t {
	case "string":
		return "STRING"
	case "number":
		return "NUMBER"
	case "integer":
		return "INTEGER"
	case "boolean":
		return "BOOLEAN"
	case "array":
		return "ARRAY"
	case "object":
		return "OBJECT"
	default:
		return "OBJECT"
	}
}

func mapGeminiFinishReason(reason string) string {
	switch reason {
	case "STOP":
		return "end_turn"
	case "MAX_TOKENS":
		return "max_tokens"
	case "SAFETY", "RECITATION", "OTHER", "BLOCKED":
		return "stop_sequence"
	default:
		return reason
	}
}

func (h *GeminiHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.resolveConfig(req)
	model := h.resolveModel(req)
	payload, err := h.buildRequestPayload(req)
	if err != nil {
		return nil, err
	}

	reqBody, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %w", err)
	}

	url := fmt.Sprintf("%s/v1beta/models/%s:generateContent?key=%s", baseURL, model, apiKey)
	httpReq, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewBuffer(reqBody))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")

	resp, err := SharedHTTPClient.Do(httpReq)
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

	return geminiConvertResponse(body, model)
}

func geminiConvertResponse(body []byte, model string) (*SendResult, error) {
	var resp struct {
		Candidates []struct {
			Content *struct {
				Parts []json.RawMessage `json:"parts"`
			} `json:"content"`
			FinishReason string `json:"finishReason"`
		} `json:"candidates"`
		UsageMetadata *struct {
			PromptTokenCount     int `json:"promptTokenCount"`
			CandidatesTokenCount int `json:"candidatesTokenCount"`
			TotalTokenCount      int `json:"totalTokenCount"`
		} `json:"usageMetadata"`
	}
	if err := json.Unmarshal(body, &resp); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}

	var contentBlocks []ContentBlock
	var stopReason string

	for _, candidate := range resp.Candidates {
		if candidate.FinishReason != "" && candidate.FinishReason != "FINISH_REASON_UNSPECIFIED" {
			stopReason = mapGeminiFinishReason(candidate.FinishReason)
		}
		if candidate.Content == nil {
			continue
		}
		for _, partRaw := range candidate.Content.Parts {
			var partProbe map[string]json.RawMessage
			if json.Unmarshal(partRaw, &partProbe) != nil {
				continue
			}
			if textRaw, ok := partProbe["text"]; ok {
				var text string
				if json.Unmarshal(textRaw, &text) == nil && text != "" {
					contentBlocks = append(contentBlocks, ContentBlock{Type: "text", Text: text})
				}
			} else if fcRaw, ok := partProbe["functionCall"]; ok {
				var fc struct {
					Name string          `json:"name"`
					Args json.RawMessage `json:"args"`
				}
				if json.Unmarshal(fcRaw, &fc) == nil {
					contentBlocks = append(contentBlocks, ContentBlock{
						Type: "tool_use",
						ToolUse: &ToolUseBlock{
							ID:   fc.Name,
							Type: "tool_use",
							Function: struct {
								Name      string `json:"name"`
								Arguments string `json:"arguments"`
							}{Name: fc.Name, Arguments: string(fc.Args)},
						},
					})
				}
			}
		}
	}

	usage := &Usage{}
	if resp.UsageMetadata != nil {
		usage.InputTokens = resp.UsageMetadata.PromptTokenCount
		usage.OutputTokens = resp.UsageMetadata.CandidatesTokenCount
		usage.TotalTokens = resp.UsageMetadata.TotalTokenCount
	}

	return &SendResult{
		Content:    contentBlocks,
		Model:      model,
		Usage:      usage,
		StopReason: stopReason,
	}, nil
}

func (h *GeminiHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.resolveConfig(req)
	model := h.resolveModel(req)
	payload, err := h.buildRequestPayload(req)
	if err != nil {
		return err
	}

	reqBody, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("failed to marshal request: %w", err)
	}

	url := fmt.Sprintf("%s/v1beta/models/%s:streamGenerateContent?alt=sse&key=%s", baseURL, model, apiKey)
	httpReq, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewBuffer(reqBody))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Accept", "text/event-stream")

	resp, err := SharedHTTPClient.Do(httpReq)
	if err != nil {
		return wrapTransientError(fmt.Errorf("request failed: %w", err))
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
		return newAPIErrorFromResp(resp, string(body))
	}

	log.Printf("[Gemini:Stream] model=%s msgs=%d tools=%d", model, len(req.Messages), len(req.Tools))

	return geminiParseSSE(ctx, &contextReader{ctx: ctx, r: resp.Body}, callback)
}

// geminiParseSSE reads a Gemini SSE stream and emits StreamChunks.
// Gemini SSE uses simple data:<json> lines without event type prefixes.
func geminiParseSSE(ctx context.Context, body io.Reader, callback func(StreamChunk) error) error {
	scanner := bufio.NewScanner(body)
	scanner.Buffer(make([]byte, 0, 32*1024), 256*1024)

	toolCallIdx := 0

	for scanner.Scan() {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		line := scanner.Text()
		if !strings.HasPrefix(line, "data: ") {
			continue
		}
		data := strings.TrimPrefix(line, "data: ")

		var chunk struct {
			Candidates []struct {
				Content *struct {
					Parts []json.RawMessage `json:"parts"`
				} `json:"content"`
				FinishReason string `json:"finishReason"`
			} `json:"candidates"`
			UsageMetadata *struct {
				PromptTokenCount     int `json:"promptTokenCount"`
				CandidatesTokenCount int `json:"candidatesTokenCount"`
				TotalTokenCount      int `json:"totalTokenCount"`
			} `json:"usageMetadata"`
		}

		if err := json.Unmarshal([]byte(data), &chunk); err != nil {
			continue
		}

		if len(chunk.Candidates) == 0 {
			continue
		}
		candidate := chunk.Candidates[0]

		if candidate.Content != nil {
			for _, partRaw := range candidate.Content.Parts {
				var partProbe map[string]json.RawMessage
				if json.Unmarshal(partRaw, &partProbe) != nil {
					continue
				}
				if textRaw, ok := partProbe["text"]; ok {
					var text string
					if json.Unmarshal(textRaw, &text) == nil && text != "" {
						if err := callback(StreamChunk{Type: "delta", TextDelta: text}); err != nil {
							return err
						}
					}
				} else if fcRaw, ok := partProbe["functionCall"]; ok {
					var fc struct {
						Name string          `json:"name"`
						Args json.RawMessage `json:"args"`
					}
					if json.Unmarshal(fcRaw, &fc) == nil {
						callID := fmt.Sprintf("gemini_%d_%s", toolCallIdx, fc.Name)
						idx := toolCallIdx
						toolCallIdx++

						if err := callback(StreamChunk{
							Type:         "content",
							Index:        idx,
							Content:      "tool_use",
							ToolCallID:   callID,
							ToolCallName: fc.Name,
						}); err != nil {
							return err
						}
						argsJSON := string(fc.Args)
						if argsJSON != "" {
							if err := callback(StreamChunk{
								Type:      "delta",
								Index:     idx,
								JSONDelta: argsJSON,
							}); err != nil {
								return err
							}
						}
					}
				}
			}
		}

		if candidate.FinishReason != "" && candidate.FinishReason != "FINISH_REASON_UNSPECIFIED" {
			usage := &Usage{}
			if chunk.UsageMetadata != nil {
				usage.InputTokens = chunk.UsageMetadata.PromptTokenCount
				usage.OutputTokens = chunk.UsageMetadata.CandidatesTokenCount
				usage.TotalTokens = chunk.UsageMetadata.TotalTokenCount
			}
			if err := callback(StreamChunk{
				Type:         "stop",
				FinishReason: mapGeminiFinishReason(candidate.FinishReason),
				Usage:        usage,
			}); err != nil {
				return err
			}
		}
	}

	return scanner.Err()
}

// ListModels fetches available Gemini models via the REST API.
func (h *GeminiHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	baseURL := "https://generativelanguage.googleapis.com"
	if cfg.BaseURL != "" {
		baseURL = cfg.BaseURL
	}
	apiKey := cfg.APIKey

	url := fmt.Sprintf("%s/v1beta/models?key=%s", baseURL, apiKey)
	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return nil, err
	}

	resp, err := SharedHTTPClient.Do(req)
	if err != nil {
		return nil, wrapTransientError(err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, err
	}

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("Gemini /models returned status %d: %s", resp.StatusCode, string(body))
	}

	var result struct {
		Models []struct {
			Name                        string   `json:"name"`
			DisplayName                 string   `json:"displayName"`
			InputTokenLimit             int      `json:"inputTokenLimit"`
			OutputTokenLimit            int      `json:"outputTokenLimit"`
			SupportedGenerationMethods  []string `json:"supportedGenerationMethods"`
		} `json:"models"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, err
	}

	entries := make([]ModelEntry, 0)
	for _, m := range result.Models {
		hasGen := false
		for _, method := range m.SupportedGenerationMethods {
			if method == "generateContent" {
				hasGen = true
				break
			}
		}
		if hasGen {
			entries = append(entries, ModelEntry{
				ID:            strings.TrimPrefix(m.Name, "models/"),
				Name:          m.DisplayName,
				ContextWindow: m.InputTokenLimit,
				MaxTokens:     m.OutputTokenLimit,
			})
		}
	}
	return entries, nil
}

func (h *GeminiHandler) Capabilities() *ProviderInfo {
	return &ProviderInfo{
		ID:               "gemini",
		DefaultModel:     "gemini-3-flash-preview",
		MaxTokensDefault: 16384,
		Features: ProviderFeatures{
			SupportsThinking:        true,
			SupportsReasoningEffort: false,
			SupportsTools:           true,
			SupportsImages:          true,
			SupportsPromptCache:     false,
			SupportsStreaming:       true,
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
		},
	}
}

func (h *GeminiHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.Capabilities(), settings, thinking)
}

var _ CapableHandler = (*GeminiHandler)(nil)
var _ SettingsValidator = (*GeminiHandler)(nil)

var _ ModelLister = (*GeminiHandler)(nil)
