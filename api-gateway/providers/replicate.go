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

// ReplicateHandler handles Replicate API requests.
// Replicate is NOT OpenAI-compatible — it uses /v1/predictions with model-specific
// input schemas, not /v1/chat/completions.
//   - Base URL: https://api.replicate.com/v1
//   - Authentication: Bearer <REPLICATE_API_TOKEN>
//   - Model format: {owner}/{model} or {owner}/{model}:{version}
//   - Endpoint: /v1/predictions (not /v1/chat/completions)
//   - Streaming: SSE via urls.stream in prediction response
//   - Parameters are model-specific; common LLM inputs are exposed as settings
type ReplicateHandler struct {
	caps *ProviderInfo
}

func NewReplicateHandler() *ReplicateHandler {
	const defaultModel = "meta/llama-3-70b-instruct"
	return &ReplicateHandler{
		caps: &ProviderInfo{
			ID:               "replicate",
			DefaultModel:     defaultModel,
			MaxTokensDefault: 4096,
			Features: ProviderFeatures{
				SupportsImages:    true,
				SupportsStreaming: true,
			},
			Settings: []ProviderSetting{
				{
					Key:         "temperature",
					Label:       "Temperature",
					Type:        SettingSlider,
					Min:         fPtr(0),
					Max:         fPtr(2),
					Step:        fPtr(0.01),
					Default:     0.7,
					Group:       "sampling",
					Description: "Controls randomness (model-specific).",
					ValidRange:  "0 – 2",
				},
				{
					Key:         "top_p",
					Label:       "Top P",
					Type:        SettingSlider,
					Min:         fPtr(0),
					Max:         fPtr(1),
					Step:        fPtr(0.01),
					Default:     0.9,
					Group:       "sampling",
					Description: "Nucleus sampling threshold (model-specific).",
					ValidRange:  "0 – 1",
				},
				{
					Key:         "system_prompt",
					Label:       "System Prompt",
					Type:        SettingText,
					Group:       "replicate",
					Description: "Override system prompt (model-specific).",
				},
				{
					Key:         "model_version",
					Label:       "Model Version",
					Type:        SettingText,
					Group:       "replicate",
					Description: "Pin to a specific model version hash.",
				},
			},
		},
	}
}

func (h *ReplicateHandler) resolveModel(req *Request) string {
	model := req.Provider.Model
	if req.ModelOverride != "" {
		model = req.ModelOverride
	}
	if model == "" {
		model = h.caps.DefaultModel
	}
	return model
}

func (h *ReplicateHandler) resolveAuth(req *Request) (baseURL, apiKey string) {
	baseURL = "https://api.replicate.com/v1"
	if req.Provider.BaseURL != "" {
		baseURL = req.Provider.BaseURL
	}
	apiKey = req.Provider.APIKey
	return
}

// buildPrompt converts messages to a chat-formatted prompt string.
func (h *ReplicateHandler) buildPrompt(req *Request) string {
	var parts []string
	for _, msg := range req.Messages {
		if msg.Content == "" {
			continue
		}
		switch msg.Role {
		case "user":
			parts = append(parts, "User: "+msg.Content)
		case "assistant":
			parts = append(parts, "Assistant: "+msg.Content)
		}
	}
	return strings.Join(parts, "\n")
}

// buildInput constructs the Replicate model-specific input map.
func (h *ReplicateHandler) buildInput(req *Request) map[string]interface{} {
	input := make(map[string]interface{})

	// System prompt: setting overrides req.System
	sysPrompt := req.System
	if sp := req.SettingString("system_prompt"); sp != "" {
		sysPrompt = sp
	}
	if sysPrompt != "" {
		input["system_prompt"] = sysPrompt
	}

	if prompt := h.buildPrompt(req); prompt != "" {
		input["prompt"] = prompt
	}

	if temp := req.SettingFloat("temperature"); temp > 0 {
		input["temperature"] = temp
	}
	if topP := req.SettingFloat("top_p"); topP > 0 {
		input["top_p"] = topP
	}
	if req.MaxTokens > 0 {
		input["max_tokens"] = req.MaxTokens
	}

	return input
}

// predictionURL returns the correct endpoint and body for a prediction request.
// When model_version is set, uses POST /v1/predictions with version hash.
// Otherwise, uses POST /v1/models/{owner}/{model}/predictions (latest version).
func (h *ReplicateHandler) predictionURL(baseURL, model string, req *Request) (url string, body map[string]interface{}) {
	input := h.buildInput(req)
	if v := req.SettingString("model_version"); v != "" {
		return baseURL + "/predictions", map[string]interface{}{
			"version": v,
			"input":   input,
		}
	}
	return baseURL + "/models/" + model + "/predictions", map[string]interface{}{
		"input": input,
	}
}

func (h *ReplicateHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	baseURL, apiKey := h.resolveAuth(req)
	model := h.resolveModel(req)

	url, body := h.predictionURL(baseURL, model, req)

	jsonData, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("replicate: marshal request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewReader(jsonData))
	if err != nil {
		return nil, fmt.Errorf("replicate: create request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}

	resp, err := SharedHTTPClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("replicate: request failed: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, fmt.Errorf("replicate: read response: %w", err)
	}

	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return nil, fmt.Errorf("replicate: API error (%d): %s", resp.StatusCode, string(respBody))
	}

	var result struct {
		Output interface{} `json:"output"`
		Status string      `json:"status"`
		Error  *struct {
			Detail string `json:"detail"`
		} `json:"error"`
	}
	if err := json.Unmarshal(respBody, &result); err != nil {
		return nil, fmt.Errorf("replicate: decode response: %w", err)
	}

	if result.Error != nil {
		return nil, fmt.Errorf("replicate: prediction error: %s", result.Error.Detail)
	}

	var content string
	switch v := result.Output.(type) {
	case string:
		content = v
	case []interface{}:
		var parts []string
		for _, item := range v {
			if s, ok := item.(string); ok {
				parts = append(parts, s)
			}
		}
		content = strings.Join(parts, "")
	default:
		if v != nil {
			b, _ := json.Marshal(v)
			content = string(b)
		}
	}

	return &SendResult{
		Content:    []ContentBlock{{Type: "text", Text: content}},
		Model:      model,
		StopReason: result.Status,
	}, nil
}

func (h *ReplicateHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	baseURL, apiKey := h.resolveAuth(req)
	model := h.resolveModel(req)

	url, body := h.predictionURL(baseURL, model, req)
	body["stream"] = true

	jsonData, err := json.Marshal(body)
	if err != nil {
		return fmt.Errorf("replicate: marshal request: %w", err)
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewReader(jsonData))
	if err != nil {
		return fmt.Errorf("replicate: create request: %w", err)
	}
	httpReq.Header.Set("Content-Type", "application/json")
	if apiKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+apiKey)
	}

	resp, err := SharedHTTPClient.Do(httpReq)
	if err != nil {
		return fmt.Errorf("replicate: request failed: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return fmt.Errorf("replicate: read response: %w", err)
	}

	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return fmt.Errorf("replicate: API error (%d): %s", resp.StatusCode, string(respBody))
	}

	var predResp struct {
		URLs struct {
			Stream string `json:"stream"`
		} `json:"urls"`
		Error *struct {
			Detail string `json:"detail"`
		} `json:"error"`
	}
	if err := json.Unmarshal(respBody, &predResp); err != nil {
		return fmt.Errorf("replicate: decode response: %w", err)
	}

	if predResp.Error != nil {
		return fmt.Errorf("replicate: prediction error: %s", predResp.Error.Detail)
	}

	if predResp.URLs.Stream == "" {
		return fmt.Errorf("replicate: no stream URL in response")
	}

	// Connect to SSE stream
	streamReq, err := http.NewRequestWithContext(ctx, "GET", predResp.URLs.Stream, nil)
	if err != nil {
		return fmt.Errorf("replicate: create stream request: %w", err)
	}
	if apiKey != "" {
		streamReq.Header.Set("Authorization", "Bearer "+apiKey)
	}
	streamReq.Header.Set("Accept", "text/event-stream")

	streamResp, err := SharedHTTPClient.Do(streamReq)
	if err != nil {
		return fmt.Errorf("replicate: stream connect failed: %w", err)
	}
	defer streamResp.Body.Close()

	if streamResp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(io.LimitReader(streamResp.Body, maxBodySize))
		return fmt.Errorf("replicate: stream error (%d): %s", streamResp.StatusCode, string(body))
	}

	// Parse Replicate SSE: event:output/error/done with data:payload
	scanner := bufio.NewScanner(streamResp.Body)
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	var eventType string
	for scanner.Scan() {
		line := scanner.Text()

		if strings.HasPrefix(line, "event:") {
			eventType = strings.TrimSpace(strings.TrimPrefix(line, "event:"))
		} else if strings.HasPrefix(line, "data:") {
			data := strings.TrimPrefix(line, "data:")
			switch eventType {
			case "output":
				if err := callback(StreamChunk{Content: data}); err != nil {
					return err
				}
			case "error":
				return fmt.Errorf("replicate: stream error: %s", data)
			case "done":
				return nil
			}
			eventType = ""
		}
	}

	return scanner.Err()
}

func (h *ReplicateHandler) Capabilities() *ProviderInfo {
	return h.caps
}

func (h *ReplicateHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.caps, settings, thinking)
}

var _ Handler = (*ReplicateHandler)(nil)
var _ CapableHandler = (*ReplicateHandler)(nil)
var _ SettingsValidator = (*ReplicateHandler)(nil)

func (h *ReplicateHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	baseURL := "https://api.replicate.com/v1"
	if cfg.BaseURL != "" {
		baseURL = cfg.BaseURL
	}

	httpReq, err := http.NewRequestWithContext(ctx, "GET", baseURL+"/models", nil)
	if err != nil {
		return nil, err
	}
	if cfg.APIKey != "" {
		httpReq.Header.Set("Authorization", "Bearer "+cfg.APIKey)
	}
	httpReq.Header.Set("Accept", "application/json")

	resp, err := SharedHTTPClient.Do(httpReq)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBodySize))
	if err != nil {
		return nil, err
	}

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("replicate: API error (%d): %s", resp.StatusCode, string(body))
	}

	var result struct {
		Results []struct {
			Owner string `json:"owner"`
			Name  string `json:"name"`
		} `json:"results"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, fmt.Errorf("replicate: decode models: %w", err)
	}

	entries := make([]ModelEntry, 0, len(result.Results))
	for _, m := range result.Results {
		entries = append(entries, ModelEntry{
			ID:   m.Owner + "/" + m.Name,
			Name: m.Name,
		})
	}
	return entries, nil
}

var _ ModelLister = (*ReplicateHandler)(nil)
