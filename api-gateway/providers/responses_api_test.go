package providers

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestResponsesBuildTools(t *testing.T) {
	// Chat Completions format tool
	ccTool := json.RawMessage(`{
		"type": "function",
		"function": {
			"name": "get_weather",
			"description": "Get weather",
			"parameters": {"type": "object", "properties": {"location": {"type": "string"}}}
		}
	}`)

	tools := responsesBuildTools([]json.RawMessage{ccTool})
	if len(tools) != 1 {
		t.Fatalf("expected 1 tool, got %d", len(tools))
	}

	tool := tools[0].(map[string]interface{})
	if tool["type"] != "function" {
		t.Errorf("expected type=function, got %v", tool["type"])
	}
	if tool["name"] != "get_weather" {
		t.Errorf("expected name=get_weather, got %v", tool["name"])
	}
	if tool["description"] != "Get weather" {
		t.Errorf("expected description, got %v", tool["description"])
	}
	if tool["parameters"] == nil {
		t.Error("expected parameters to be set")
	}
}

func TestResponsesConvertMessages(t *testing.T) {
	req := &Request{
		System: "You are helpful.",
		Messages: []Message{
			{Role: "user", Content: "Hello"},
			{Role: "assistant", Content: "Hi there"},
			{Role: "user", Content: "What is 2+2?"},
		},
	}

	items := responsesConvertMessages(req)

	// System message is NOT in items (it goes to instructions)
	// We expect 3 items (3 messages)
	if len(items) != 3 {
		t.Fatalf("expected 3 items, got %d", len(items))
	}

	// First item: user message
	item0 := items[0].(map[string]interface{})
	if item0["type"] != "message" {
		t.Errorf("item 0: expected type=message, got %v", item0["type"])
	}
	if item0["role"] != "user" {
		t.Errorf("item 0: expected role=user, got %v", item0["role"])
	}

	// Second item: assistant message
	item1 := items[1].(map[string]interface{})
	if item1["type"] != "message" {
		t.Errorf("item 1: expected type=message, got %v", item1["type"])
	}
	if item1["role"] != "assistant" {
		t.Errorf("item 1: expected role=assistant, got %v", item1["role"])
	}
}

func TestResponsesConvertToolResult(t *testing.T) {
	req := &Request{
		Messages: []Message{
			{
				Role: "user",
				ToolResult: &ToolResult{
					ToolUseID: "call_123",
					Content:   "4",
				},
			},
		},
	}

	items := responsesConvertMessages(req)
	if len(items) != 1 {
		t.Fatalf("expected 1 item, got %d", len(items))
	}

	item := items[0].(map[string]interface{})
	if item["type"] != "function_call_output" {
		t.Errorf("expected type=function_call_output, got %v", item["type"])
	}
	if item["call_id"] != "call_123" {
		t.Errorf("expected call_id=call_123, got %v", item["call_id"])
	}
	if item["output"] != "4" {
		t.Errorf("expected output=4, got %v", item["output"])
	}
}

func TestResponsesConvertToolCalls(t *testing.T) {
	req := &Request{
		Messages: []Message{
			{
				Role: "assistant",
				ToolCalls: []ToolCall{
					{
						ID:   "call_456",
						Type: "function",
						Function: FunctionCall{
							Name:      "calc",
							Arguments: `{"expr": "2+2"}`,
						},
					},
				},
			},
		},
	}

	items := responsesConvertMessages(req)
	if len(items) != 1 {
		t.Fatalf("expected 1 item, got %d", len(items))
	}

	item := items[0].(map[string]interface{})
	if item["type"] != "function_call" {
		t.Errorf("expected type=function_call, got %v", item["type"])
	}
	if item["call_id"] != "call_456" {
		t.Errorf("expected call_id=call_456, got %v", item["call_id"])
	}
	if item["name"] != "calc" {
		t.Errorf("expected name=calc, got %v", item["name"])
	}
}

func TestResponsesParseResponse(t *testing.T) {
	handler := newResponsesAPIHandler(ResponsesAPIConfig{
		BaseURL:      "https://api.openai.com/v1",
		DefaultModel: "gpt-5-codex-mini",
	})

	body := []byte(`{
		"id": "resp_123",
		"object": "response",
		"status": "completed",
		"model": "gpt-5-codex-mini",
		"output": [
			{
				"type": "message",
				"role": "assistant",
				"content": [
					{"type": "output_text", "text": "The answer is 4."}
				]
			}
		],
		"usage": {
			"input_tokens": 10,
			"output_tokens": 5,
			"input_tokens_details": {"cached_tokens": 3},
			"output_tokens_details": {"reasoning_tokens": 2}
		}
	}`)

	result, err := handler.parseResponse(body)
	if err != nil {
		t.Fatalf("parseResponse error: %v", err)
	}

	if result.StopReason != "stop" {
		t.Errorf("expected stop_reason=stop, got %s", result.StopReason)
	}
	if result.Model != "gpt-5-codex-mini" {
		t.Errorf("expected model=gpt-5-codex-mini, got %s", result.Model)
	}
	if len(result.Content) != 1 {
		t.Fatalf("expected 1 content block, got %d", len(result.Content))
	}
	if result.Content[0].Type != "text" {
		t.Errorf("expected type=text, got %s", result.Content[0].Type)
	}
	if result.Content[0].Text != "The answer is 4." {
		t.Errorf("expected text='The answer is 4.', got %s", result.Content[0].Text)
	}
	if result.Usage == nil {
		t.Fatal("expected usage to be set")
	}
	if result.Usage.InputTokens != 10 {
		t.Errorf("expected input_tokens=10, got %d", result.Usage.InputTokens)
	}
	if result.Usage.OutputTokens != 5 {
		t.Errorf("expected output_tokens=5, got %d", result.Usage.OutputTokens)
	}
	if result.Usage.CacheReadInputTokens != 3 {
		t.Errorf("expected cached_tokens=3, got %d", result.Usage.CacheReadInputTokens)
	}
	if result.Usage.ReasoningTokens != 2 {
		t.Errorf("expected reasoning_tokens=2, got %d", result.Usage.ReasoningTokens)
	}
}

func TestResponsesParseFunctionCall(t *testing.T) {
	handler := newResponsesAPIHandler(ResponsesAPIConfig{
		BaseURL:      "https://api.openai.com/v1",
		DefaultModel: "gpt-5-codex-mini",
	})

	body := []byte(`{
		"id": "resp_456",
		"object": "response",
		"status": "completed",
		"model": "gpt-5-codex-mini",
		"output": [
			{
				"type": "function_call",
				"call_id": "call_abc",
				"name": "get_weather",
				"arguments": "{\"location\": \"SF\"}"
			}
		],
		"usage": {"input_tokens": 8, "output_tokens": 12}
	}`)

	result, err := handler.parseResponse(body)
	if err != nil {
		t.Fatalf("parseResponse error: %v", err)
	}

	if len(result.Content) != 1 {
		t.Fatalf("expected 1 content block, got %d", len(result.Content))
	}
	cb := result.Content[0]
	if cb.Type != "tool_use" {
		t.Errorf("expected type=tool_use, got %s", cb.Type)
	}
	if cb.ToolUse == nil {
		t.Fatal("expected tool_use to be set")
	}
	if cb.ToolUse.ID != "call_abc" {
		t.Errorf("expected id=call_abc, got %s", cb.ToolUse.ID)
	}
	if cb.ToolUse.Function.Name != "get_weather" {
		t.Errorf("expected name=get_weather, got %s", cb.ToolUse.Function.Name)
	}
	if cb.ToolUse.Function.Arguments != `{"location": "SF"}` {
		t.Errorf("unexpected arguments: %s", cb.ToolUse.Function.Arguments)
	}
}

func TestResponsesBuildRequest(t *testing.T) {
	handler := newResponsesAPIHandler(ResponsesAPIConfig{
		BaseURL:      "https://api.openai.com/v1",
		DefaultModel: "gpt-5-codex-mini",
	})

	req := &Request{
		System:      "Be helpful",
		Temperature: 0.7,
		TopP:        0.9,
		MaxTokens:   4096,
		Thinking: &ThinkingConfig{
			Type:            "enabled",
			BudgetTokens:    10000,
			ReasoningEffort: "high",
		},
		Provider: ProviderConfig{ID: "openai_codex"},
	}

	result := handler.buildRequest(req, true)

	if result["model"] != "gpt-5-codex-mini" {
		t.Errorf("expected model, got %v", result["model"])
	}
	if result["instructions"] != "Be helpful" {
		t.Errorf("expected instructions='Be helpful', got %v", result["instructions"])
	}
	if result["temperature"] != 0.7 {
		t.Errorf("expected temperature=0.7, got %v", result["temperature"])
	}
	if result["top_p"] != 0.9 {
		t.Errorf("expected top_p=0.9, got %v", result["top_p"])
	}
	if result["max_output_tokens"] != 4096 {
		t.Errorf("expected max_output_tokens=4096, got %v", result["max_output_tokens"])
	}
	if result["store"] != false {
		t.Errorf("expected store=false, got %v", result["store"])
	}
	if result["stream"] != true {
		t.Errorf("expected stream=true, got %v", result["stream"])
	}
	if reasoning, ok := result["reasoning"].(map[string]interface{}); !ok {
		t.Error("expected reasoning to be set")
	} else {
		if reasoning["effort"] != "high" {
			t.Errorf("expected reasoning effort=high, got %v", reasoning["effort"])
		}
	}
	if inc, ok := result["include"].([]string); !ok || len(inc) != 1 || inc[0] != "reasoning.encrypted_content" {
		t.Errorf("expected include=[reasoning.encrypted_content], got %v", result["include"])
	}
}

func TestResponsesHandlerSendWithMockServer(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/responses" {
			t.Errorf("expected /responses, got %s", r.URL.Path)
		}
		if r.Method != "POST" {
			t.Errorf("expected POST, got %s", r.Method)
		}
		if r.Header.Get("Authorization") != "Bearer test-token" {
			t.Errorf("expected Bearer test-token, got %s", r.Header.Get("Authorization"))
		}

		resp := map[string]interface{}{
			"id":     "resp_test",
			"object": "response",
			"model":  "gpt-5-codex-mini",
			"output": []interface{}{
				map[string]interface{}{
					"type": "message",
					"role": "assistant",
					"content": []interface{}{
						map[string]interface{}{"type": "output_text", "text": "Hello!"},
					},
				},
			},
			"usage": map[string]interface{}{
				"input_tokens":  5,
				"output_tokens": 2,
			},
		}
		json.NewEncoder(w).Encode(resp)
	}))
	defer srv.Close()

	handler := newResponsesAPIHandler(ResponsesAPIConfig{
		BaseURL:      srv.URL,
		DefaultModel: "gpt-5-codex-mini",
	})

	req := &Request{
		Provider: ProviderConfig{
			ID:      "openai_codex",
			APIKey:  "test-token",
			BaseURL: srv.URL,
		},
		Messages: []Message{
			{Role: "user", Content: "Hi"},
		},
	}

	result, err := handler.Send(rContext(t), req)
	if err != nil {
		t.Fatalf("Send error: %v", err)
	}
	if len(result.Content) != 1 {
		t.Fatalf("expected 1 content block, got %d", len(result.Content))
	}
	if result.Content[0].Text != "Hello!" {
		t.Errorf("expected 'Hello!', got %s", result.Content[0].Text)
	}
	if result.Usage.InputTokens != 5 || result.Usage.OutputTokens != 2 {
		t.Errorf("usage mismatch: %+v", result.Usage)
	}
}

func rContext(t *testing.T) context.Context {
	t.Helper()
	return context.TODO()
}
