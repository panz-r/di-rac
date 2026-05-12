package providers

import (
	"context"
	"encoding/json"
	"testing"
)

// MockHandler is a mock implementation of the Handler interface for testing
type MockHandler struct {
	SendFunc     func(ctx context.Context, req *Request) (*SendResult, error)
	StreamFunc   func(ctx context.Context, req *Request, callback func(StreamChunk) error) error
	SendCalled   int
	StreamCalled int
}

func (h *MockHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	h.SendCalled++
	if h.SendFunc != nil {
		return h.SendFunc(ctx, req)
	}
	return &SendResult{}, nil
}

func (h *MockHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	h.StreamCalled++
	if h.StreamFunc != nil {
		return h.StreamFunc(ctx, req, callback)
	}
	return nil
}

func TestNewRegistry(t *testing.T) {
	registry := NewRegistry()
	if registry == nil {
		t.Fatal("NewRegistry returned nil")
	}

	providers := registry.SupportedProviders()
	if len(providers) == 0 {
		t.Error("Expected at least one provider to be registered")
	}
}

func TestRegistryRegister(t *testing.T) {
	registry := NewRegistry()
	mockHandler := &MockHandler{}
	registry.Register("testprovider", mockHandler, ProviderMeta{ID: "testprovider", Label: "Test"})

	handler, err := registry.GetHandler("testprovider")
	if err != nil {
		t.Fatalf("Failed to get handler: %v", err)
	}

	if handler != mockHandler {
		t.Error("Handler mismatch")
	}
}

func TestRegistryGetHandlerNotFound(t *testing.T) {
	registry := NewRegistry()
	_, err := registry.GetHandler("nonexistent")
	if err == nil {
		t.Error("Expected error for non-existent provider")
	}
}

func TestRegistrySupportedProviders(t *testing.T) {
	registry := NewRegistry()
	providers := registry.SupportedProviders()

	found := false
	for _, p := range providers {
		if p.ID == "anthropic" {
			found = true
			break
		}
	}
	if !found {
		t.Error("Expected 'anthropic' provider to be registered")
	}
}

func TestValidateRequest(t *testing.T) {
	// valid request with legacy Content field
	req1 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{{Role: "user", Content: "hello"}},
	}
	if err := ValidateRequest(req1); err != nil {
		t.Errorf("valid request failed: %v", err)
	}

	// missing provider ID
	req2 := &Request{
		Messages: []Message{{Role: "user", Content: "hello"}},
	}
	if err := ValidateRequest(req2); err == nil {
		t.Error("missing provider ID should fail")
	}

	// empty messages
	req3 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{},
	}
	if err := ValidateRequest(req3); err == nil {
		t.Error("empty messages should fail")
	}

	// message without role
	req4 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{{Content: "hello"}},
	}
	if err := ValidateRequest(req4); err == nil {
		t.Error("missing role should fail")
	}

	// message without content
	req5 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{{Role: "user"}},
	}
	if err := ValidateRequest(req5); err == nil {
		t.Error("missing content should fail")
	}

	// message with tool calls is valid
	req6 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{
			{Role: "assistant", ToolCalls: []ToolCall{
				{ID: "1", Type: "function", Function: FunctionCall{Name: "test", Arguments: "{}"}},
			}},
		},
	}
	if err := ValidateRequest(req6); err != nil {
		t.Errorf("tool calls should be valid: %v", err)
	}

	// message with tool result is valid (must follow a tool_use)
	req7 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{
			{Role: "assistant", ToolCalls: []ToolCall{
				{ID: "1", Type: "function", Function: FunctionCall{Name: "test", Arguments: "{}"}},
			}},
			{Role: "user", ToolResult: &ToolResult{ToolUseID: "1", Content: "result"}},
		},
	}
	if err := ValidateRequest(req7); err != nil {
		t.Errorf("tool result should be valid: %v", err)
	}
}

// TestValidateRequestWithContentBlocks tests 3.1: ContentBlocks in Message
func TestValidateRequestWithContentBlocks(t *testing.T) {
	// valid request with ContentBlocks
	req1 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{
			{
				Role: "user",
				ContentBlocks: []ContentBlock{
					{Type: "text", Text: "Hello"},
				},
			},
		},
	}
	if err := ValidateRequest(req1); err != nil {
		t.Errorf("ContentBlocks request failed: %v", err)
	}

	// valid request with Thinking content
	req2 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{
			{
				Role:     "assistant",
				Thinking: "Let me think about this...",
			},
		},
	}
	if err := ValidateRequest(req2); err != nil {
		t.Errorf("Thinking request failed: %v", err)
	}

	// valid request with mixed ContentBlocks (text + thinking + image)
	req3 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{
			{
				Role: "user",
				ContentBlocks: []ContentBlock{
					{Type: "text", Text: "What's in this image?"},
					{Type: "image", ImageSource: &ImageSourceBlock{
						Type:     "image/jpeg",
						Data:     "base64encodeddata",
						MimeType: "image/jpeg",
					}},
				},
			},
		},
	}
	if err := ValidateRequest(req3); err != nil {
		t.Errorf("mixed ContentBlocks request failed: %v", err)
	}

	// valid request with tool_use and tool_result ContentBlocks
	req4 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{
			{
				Role: "assistant",
				ContentBlocks: []ContentBlock{
					{
						Type: "tool_use",
						ToolUse: &ToolUseBlock{
							ID:   "tool_1",
							Type: "tool_use",
							Function: struct {
								Name      string `json:"name"`
								Arguments string `json:"arguments"`
							}{
								Name:      "get_weather",
								Arguments: "{\"city\":\"NYC\"}",
							},
						},
					},
				},
			},
		},
	}
	if err := ValidateRequest(req4); err != nil {
		t.Errorf("tool_use ContentBlock request failed: %v", err)
	}

	req5 := &Request{
		Provider: ProviderConfig{ID: "anthropic"},
		Messages: []Message{
			{
				Role: "assistant",
				ContentBlocks: []ContentBlock{
					{
						Type: "tool_use",
						ToolUse: &ToolUseBlock{
							ID:   "tool_1",
							Type: "tool_use",
							Function: struct {
								Name      string `json:"name"`
								Arguments string `json:"arguments"`
							}{
								Name:      "get_weather",
								Arguments: "{\"city\":\"NYC\"}",
							},
						},
					},
				},
			},
			{
				Role: "user",
				ContentBlocks: []ContentBlock{
					{
						Type: "tool_result",
						ToolResult: &ToolResultBlock{
							ToolUseID: "tool_1",
							Content:   "Sunny, 72F",
							IsError:   false,
						},
					},
				},
			},
		},
	}
	if err := ValidateRequest(req5); err != nil {
		t.Errorf("tool_result ContentBlock request failed: %v", err)
	}
}

func TestMessageWithToolCalls(t *testing.T) {
	msgJSON := `{"role": "assistant", "tool_calls": [{"id": "call_123", "type": "function", "function": {"name": "get_weather", "arguments": "{\"location\":\"NYC\"}"}}]}`

	var msg Message
	if err := json.Unmarshal([]byte(msgJSON), &msg); err != nil {
		t.Fatalf("Failed to unmarshal message: %v", err)
	}

	if msg.Role != "assistant" {
		t.Errorf("Expected role 'assistant', got '%s'", msg.Role)
	}
	if len(msg.ToolCalls) != 1 {
		t.Fatalf("Expected 1 tool call, got %d", len(msg.ToolCalls))
	}
	if msg.ToolCalls[0].Function.Name != "get_weather" {
		t.Errorf("Expected function name 'get_weather', got '%s'", msg.ToolCalls[0].Function.Name)
	}
}

func TestMessageWithToolResult(t *testing.T) {
	msgJSON := `{"role": "user", "tool_use_id": "tool_123", "tool_result": {"tool_use_id": "call_123", "content": "Sunny, 72F", "is_error": false}}`

	var msg Message
	if err := json.Unmarshal([]byte(msgJSON), &msg); err != nil {
		t.Fatalf("Failed to unmarshal message: %v", err)
	}

	if msg.ToolResult == nil {
		t.Fatal("Expected tool result to be non-nil")
	}
	if msg.ToolResult.Content != "Sunny, 72F" {
		t.Errorf("Unexpected content: %s", msg.ToolResult.Content)
	}
}

func TestThinkingConfig(t *testing.T) {
	thinkingJSON := `{"type": "enabled", "budget_tokens": 10000}`

	var thinking ThinkingConfig
	if err := json.Unmarshal([]byte(thinkingJSON), &thinking); err != nil {
		t.Fatalf("Failed to unmarshal thinking config: %v", err)
	}

	if thinking.Type != "enabled" {
		t.Errorf("Expected type 'enabled', got '%s'", thinking.Type)
	}
	if thinking.BudgetTokens != 10000 {
		t.Errorf("Expected BudgetTokens 10000, got %d", thinking.BudgetTokens)
	}
}

// TestStreamChunk tests 3.3: Extended StreamChunk for structured deltas
func TestStreamChunk(t *testing.T) {
	// Test legacy content field
	chunkJSON := `{"type": "content", "index": 0, "content": "Hello world"}`

	var chunk StreamChunk
	if err := json.Unmarshal([]byte(chunkJSON), &chunk); err != nil {
		t.Fatalf("Failed to unmarshal stream chunk: %v", err)
	}

	if chunk.Type != "content" {
		t.Errorf("Expected type 'content', got '%s'", chunk.Type)
	}
	if chunk.Index != 0 {
		t.Errorf("Expected index 0, got %d", chunk.Index)
	}

	// Test typed TextDelta field
	textDeltaJSON := `{"type": "delta", "index": 0, "text_delta": "Hello"}`

	var textChunk StreamChunk
	if err := json.Unmarshal([]byte(textDeltaJSON), &textChunk); err != nil {
		t.Fatalf("Failed to unmarshal text delta chunk: %v", err)
	}

	if textChunk.TextDelta != "Hello" {
		t.Errorf("Expected text_delta 'Hello', got '%s'", textChunk.TextDelta)
	}

	// Test typed Thinking field
	thinkingDeltaJSON := `{"type": "delta", "index": 0, "thinking": "Let me think..."}`

	var thinkingChunk StreamChunk
	if err := json.Unmarshal([]byte(thinkingDeltaJSON), &thinkingChunk); err != nil {
		t.Fatalf("Failed to unmarshal thinking chunk: %v", err)
	}

	if thinkingChunk.Thinking != "Let me think..." {
		t.Errorf("Expected thinking 'Let me think...', got '%s'", thinkingChunk.Thinking)
	}

	// Test JSONDelta field
	jsonDeltaJSON := `{"type": "delta", "index": 0, "json_delta": "{\"command\":\"ls\"}"}`

	var jsonChunk StreamChunk
	if err := json.Unmarshal([]byte(jsonDeltaJSON), &jsonChunk); err != nil {
		t.Fatalf("Failed to unmarshal json delta chunk: %v", err)
	}

	if jsonChunk.JSONDelta == "" {
		t.Fatal("Expected json_delta to be non-empty")
	}

	// Test Usage field
	usageJSON := `{"type": "stop", "finish_reason": "stop", "usage": {"input_tokens": 100, "output_tokens": 50, "total_tokens": 150}}`

	var usageChunk StreamChunk
	if err := json.Unmarshal([]byte(usageJSON), &usageChunk); err != nil {
		t.Fatalf("Failed to unmarshal usage chunk: %v", err)
	}

	if usageChunk.Usage == nil {
		t.Fatal("Expected usage to be non-nil")
	}
	if usageChunk.Usage.InputTokens != 100 {
		t.Errorf("Expected input_tokens 100, got %d", usageChunk.Usage.InputTokens)
	}
	if usageChunk.Usage.OutputTokens != 50 {
		t.Errorf("Expected output_tokens 50, got %d", usageChunk.Usage.OutputTokens)
	}
	if usageChunk.Usage.TotalTokens != 150 {
		t.Errorf("Expected total_tokens 150, got %d", usageChunk.Usage.TotalTokens)
	}

	// Test FinishReason field
	if usageChunk.FinishReason != "stop" {
		t.Errorf("Expected finish_reason 'stop', got '%s'", usageChunk.FinishReason)
	}

	// Test ContentBlocks in StreamChunk
	contentBlocksJSON := `{"type": "stop", "content_blocks": [{"type": "text", "text": "final response"}]}`

	var cbChunk StreamChunk
	if err := json.Unmarshal([]byte(contentBlocksJSON), &cbChunk); err != nil {
		t.Fatalf("Failed to unmarshal content blocks chunk: %v", err)
	}

	if len(cbChunk.ContentBlocks) != 1 {
		t.Fatalf("Expected 1 content block, got %d", len(cbChunk.ContentBlocks))
	}
	if cbChunk.ContentBlocks[0].Text != "final response" {
		t.Errorf("Expected text 'final response', got '%s'", cbChunk.ContentBlocks[0].Text)
	}
}

// TestContentBlockAllTypes tests 3.2: Extended ContentBlock for all types
func TestContentBlockAllTypes(t *testing.T) {
	// Test text content block
	textJSON := `{"type": "text", "text": "Hello, world!"}`
	var textCB ContentBlock
	if err := json.Unmarshal([]byte(textJSON), &textCB); err != nil {
		t.Fatalf("Failed to unmarshal text content block: %v", err)
	}
	if textCB.Type != "text" {
		t.Errorf("Expected type 'text', got '%s'", textCB.Type)
	}
	if textCB.Text != "Hello, world!" {
		t.Errorf("Expected text 'Hello, world!', got '%s'", textCB.Text)
	}

	// Test thinking content block
	thinkingJSON := `{"type": "thinking", "thinking": "Let me analyze this...", "signature": "sig_123"}`
	var thinkingCB ContentBlock
	if err := json.Unmarshal([]byte(thinkingJSON), &thinkingCB); err != nil {
		t.Fatalf("Failed to unmarshal thinking content block: %v", err)
	}
	if thinkingCB.Type != "thinking" {
		t.Errorf("Expected type 'thinking', got '%s'", thinkingCB.Type)
	}
	if thinkingCB.Thinking != "Let me analyze this..." {
		t.Errorf("Expected thinking 'Let me analyze this...', got '%s'", thinkingCB.Thinking)
	}
	if thinkingCB.Signature != "sig_123" {
		t.Errorf("Expected signature 'sig_123', got '%s'", thinkingCB.Signature)
	}

	// Test image_source content block
	imageJSON := `{"role": "user", "content_blocks": [{"type": "image", "image_source": {"type": "image/jpeg", "data": "base64data...", "mime_type": "image/jpeg"}}]}`
	var imageMsg Message
	if err := json.Unmarshal([]byte(imageJSON), &imageMsg); err != nil {
		t.Fatalf("Failed to unmarshal image content block: %v", err)
	}
	if imageMsg.ContentBlocks[0].Type != "image" {
		t.Errorf("Expected type 'image', got '%s'", imageMsg.ContentBlocks[0].Type)
	}
	if imageMsg.ContentBlocks[0].ImageSource == nil {
		t.Fatal("Expected image_source to be non-nil")
	}
	if imageMsg.ContentBlocks[0].ImageSource.Data != "base64data..." {
		t.Errorf("Expected data 'base64data...', got '%s'", imageMsg.ContentBlocks[0].ImageSource.Data)
	}

	// Test tool_use content block
	toolUseJSON := `{"type": "tool_use", "tool_use": {"id": "tool_1", "type": "tool_use", "function": {"name": "get_weather", "arguments": "{}"}}}`
	var toolUseCB ContentBlock
	if err := json.Unmarshal([]byte(toolUseJSON), &toolUseCB); err != nil {
		t.Fatalf("Failed to unmarshal tool_use content block: %v", err)
	}
	if toolUseCB.ToolUse == nil {
		t.Fatal("Expected tool_use to be non-nil")
	}
	if toolUseCB.ToolUse.ID != "tool_1" {
		t.Errorf("Expected id 'tool_1', got '%s'", toolUseCB.ToolUse.ID)
	}
	if toolUseCB.ToolUse.Function.Name != "get_weather" {
		t.Errorf("Expected function name 'get_weather', got '%s'", toolUseCB.ToolUse.Function.Name)
	}

	// Test tool_result content block
	toolResultJSON := `{"type": "tool_result", "tool_result": {"type": "tool_result", "tool_use_id": "tool_1", "content": "Sunny, 72F", "is_error": false}}`
	var toolResultCB ContentBlock
	if err := json.Unmarshal([]byte(toolResultJSON), &toolResultCB); err != nil {
		t.Fatalf("Failed to unmarshal tool_result content block: %v", err)
	}
	if toolResultCB.ToolResult == nil {
		t.Fatal("Expected tool_result to be non-nil")
	}
	if toolResultCB.ToolResult.Content != "Sunny, 72F" {
		t.Errorf("Expected content 'Sunny, 72F', got '%s'", toolResultCB.ToolResult.Content)
	}
	if toolResultCB.ToolResult.IsError {
		t.Error("Expected is_error to be false")
	}

	// Test signature content block
	signatureJSON := `{"type": "signature", "signature": "sig_abc123"}`
	var signatureCB ContentBlock
	if err := json.Unmarshal([]byte(signatureJSON), &signatureCB); err != nil {
		t.Fatalf("Failed to unmarshal signature content block: %v", err)
	}
	if signatureCB.Signature != "sig_abc123" {
		t.Errorf("Expected signature 'sig_abc123', got '%s'", signatureCB.Signature)
	}
}

func TestProviderError(t *testing.T) {
	errJSON := `{"type": "rate_limit", "message": "Rate limit exceeded", "code": 429}`

	var err ProviderError
	if err := json.Unmarshal([]byte(errJSON), &err); err != nil {
		t.Fatalf("Failed to unmarshal provider error: %v", err)
	}

	if err.Type != "rate_limit" {
		t.Errorf("Expected type 'rate_limit', got '%s'", err.Type)
	}
	if err.Code != 429 {
		t.Errorf("Expected code 429, got %d", err.Code)
	}
}

func TestRegistryReRegister(t *testing.T) {
	registry := NewRegistry()

	handler1, _ := registry.GetHandler("anthropic")

	mockHandler := &MockHandler{}
	registry.Register("anthropic", mockHandler, ProviderMeta{ID: "anthropic", Label: "Anthropic"})

	handler2, _ := registry.GetHandler("anthropic")

	if handler2 != mockHandler {
		t.Error("Handler was not updated after re-register")
	}
	if handler1 == handler2 {
		t.Error("Handler should have been replaced")
	}
}

func TestHandlerErrors(t *testing.T) {
	registry := NewRegistry()

	_, err := registry.GetHandler("nonexistent")
	if err == nil {
		t.Error("Expected error for non-existent provider")
	}

	errMsg := err.Error()
	if errMsg == "" {
		t.Error("Error message should not be empty")
	}
}

func TestMockHandlerSend(t *testing.T) {
	handler := &MockHandler{}

	req := &Request{
		Provider: ProviderConfig{ID: "test"},
		Messages: []Message{{Role: "user", Content: "test"}},
	}

	result, err := handler.Send(context.Background(), req)
	if err != nil {
		t.Fatalf("Send failed: %v", err)
	}

	if handler.SendCalled != 1 {
		t.Errorf("Expected SendCalled = 1, got %d", handler.SendCalled)
	}

	_ = result
}

func TestMockHandlerSendWithFunc(t *testing.T) {
	expectedResult := &SendResult{StopReason: "test"}

	handler := &MockHandler{
		SendFunc: func(ctx context.Context, req *Request) (*SendResult, error) {
			return expectedResult, nil
		},
	}

	result, err := handler.Send(context.Background(), &Request{})
	if err != nil {
		t.Fatalf("Send failed: %v", err)
	}

	if result != expectedResult {
		t.Error("Unexpected result")
	}
}

func TestMockHandlerStream(t *testing.T) {
	handler := &MockHandler{}

	err := handler.Stream(context.Background(), &Request{}, func(chunk StreamChunk) error {
		return nil
	})

	if err != nil {
		t.Fatalf("Stream failed: %v", err)
	}

	if handler.StreamCalled != 1 {
		t.Errorf("Expected StreamCalled = 1, got %d", handler.StreamCalled)
	}
}

func TestMockHandlerStreamWithFunc(t *testing.T) {
	called := false
	handler := &MockHandler{
		StreamFunc: func(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
			called = true
			return callback(StreamChunk{Type: "test"})
		},
	}

	var receivedChunk StreamChunk
	err := handler.Stream(context.Background(), &Request{}, func(chunk StreamChunk) error {
		receivedChunk = chunk
		return nil
	})

	if err != nil {
		t.Fatalf("Stream failed: %v", err)
	}

	if !called {
		t.Error("StreamFunc was not called")
	}

	if receivedChunk.Type != "test" {
		t.Errorf("Unexpected chunk type: %s", receivedChunk.Type)
	}
}

// TestUsageFields tests extended Usage fields including cache and reasoning tokens
func TestUsageFields(t *testing.T) {
	usageJSON := `{"input_tokens": 100, "output_tokens": 50, "total_tokens": 150, "cache_creation_input_tokens": 200, "cache_read_input_tokens": 300, "reasoning_tokens": 25}`

	var usage Usage
	if err := json.Unmarshal([]byte(usageJSON), &usage); err != nil {
		t.Fatalf("Failed to unmarshal usage: %v", err)
	}

	if usage.InputTokens != 100 {
		t.Errorf("Expected input_tokens 100, got %d", usage.InputTokens)
	}
	if usage.OutputTokens != 50 {
		t.Errorf("Expected output_tokens 50, got %d", usage.OutputTokens)
	}
	if usage.TotalTokens != 150 {
		t.Errorf("Expected total_tokens 150, got %d", usage.TotalTokens)
	}
	if usage.CacheCreationInputTokens != 200 {
		t.Errorf("Expected cache_creation_input_tokens 200, got %d", usage.CacheCreationInputTokens)
	}
	if usage.CacheReadInputTokens != 300 {
		t.Errorf("Expected cache_read_input_tokens 300, got %d", usage.CacheReadInputTokens)
	}
	if usage.ReasoningTokens != 25 {
		t.Errorf("Expected reasoning_tokens 25, got %d", usage.ReasoningTokens)
	}
}

// TestImageSourceBlock tests ImageSourceBlock fields
func TestImageSourceBlock(t *testing.T) {
	// Test with data (base64)
	imageWithData := `{"role": "user", "content_blocks": [{"type": "image", "image_source": {"type": "image/jpeg", "data": "base64data...", "mime_type": "image/jpeg"}}]}`
	
	var msgWithImage Message
	if err := json.Unmarshal([]byte(imageWithData), &msgWithImage); err != nil {
		t.Fatalf("Failed to unmarshal image message: %v", err)
	}
	
	if len(msgWithImage.ContentBlocks) != 1 {
		t.Fatalf("Expected 1 content block, got %d", len(msgWithImage.ContentBlocks))
	}
	if msgWithImage.ContentBlocks[0].ImageSource == nil {
		t.Fatal("Expected image_source to be non-nil")
	}
	if msgWithImage.ContentBlocks[0].ImageSource.Data != "base64data..." {
		t.Errorf("Expected data 'base64data...', got '%s'", msgWithImage.ContentBlocks[0].ImageSource.Data)
	}

	// Test with URL
	imageWithURL := `{"role": "user", "content_blocks": [{"type": "image", "image_source": {"type": "image/png", "url": "https://example.com/image.png"}}]}`
	
	var msgWithURL Message
	if err := json.Unmarshal([]byte(imageWithURL), &msgWithURL); err != nil {
		t.Fatalf("Failed to unmarshal image with URL: %v", err)
	}
	
	if msgWithURL.ContentBlocks[0].ImageSource.URL != "https://example.com/image.png" {
		t.Errorf("Expected url 'https://example.com/image.png', got '%s'", msgWithURL.ContentBlocks[0].ImageSource.URL)
	}
}

// TestSendResultWithContentBlocks tests SendResult with ContentBlocks
func TestSendResultWithContentBlocks(t *testing.T) {
	resultJSON := `{"content": [{"type": "text", "text": "Hello"}, {"type": "thinking", "thinking": "Thinking..."}], "stop_reason": "end_turn", "usage": {"input_tokens": 10, "output_tokens": 20, "total_tokens": 30}, "model": "claude-3-5-sonnet"}`

	var result SendResult
	if err := json.Unmarshal([]byte(resultJSON), &result); err != nil {
		t.Fatalf("Failed to unmarshal send result: %v", err)
	}

	if len(result.Content) != 2 {
		t.Fatalf("Expected 2 content blocks, got %d", len(result.Content))
	}
	if result.Content[0].Type != "text" {
		t.Errorf("Expected first block type 'text', got '%s'", result.Content[0].Type)
	}
	if result.Content[0].Text != "Hello" {
		t.Errorf("Expected first block text 'Hello', got '%s'", result.Content[0].Text)
	}
	if result.Content[1].Type != "thinking" {
		t.Errorf("Expected second block type 'thinking', got '%s'", result.Content[1].Type)
	}
	if result.Content[1].Thinking != "Thinking..." {
		t.Errorf("Expected second block thinking 'Thinking...', got '%s'", result.Content[1].Thinking)
	}
	if result.StopReason != "end_turn" {
		t.Errorf("Expected stop_reason 'end_turn', got '%s'", result.StopReason)
	}
	if result.Model != "claude-3-5-sonnet" {
		t.Errorf("Expected model 'claude-3-5-sonnet', got '%s'", result.Model)
	}
	if result.Usage == nil {
		t.Fatal("Expected usage to be non-nil")
	}
	if result.Usage.TotalTokens != 30 {
		t.Errorf("Expected total_tokens 30, got %d", result.Usage.TotalTokens)
	}
}
