package providers

import (
	"sync/atomic"
	"testing"
)

// --- isRateLimitError ---

func TestIsRateLimitError_429(t *testing.T) {
	err := &ProviderAPIError{StatusCode: 429, Message: "rate limit exceeded"}
	if !isRateLimitError(err) {
		t.Error("expected true for 429 ProviderAPIError")
	}
}

func TestIsRateLimitError_RateLimitString(t *testing.T) {
	err := &ProviderAPIError{StatusCode: 500, Message: "rate limit exceeded"}
	if !isRateLimitError(err) {
		t.Error("expected true for message containing 'rate limit'")
	}
}

func TestIsRateLimitError_Unrelated(t *testing.T) {
	err := &ProviderAPIError{StatusCode: 400, Message: "bad request"}
	if isRateLimitError(err) {
		t.Error("expected false for bad request")
	}
}

func TestIsRateLimitError_Nil(t *testing.T) {
	if isRateLimitError(nil) {
		t.Error("expected false for nil")
	}
}

// --- minimaxToolCallPipe ---

func TestMiniMaxPipe_FlushEmptyNoop(t *testing.T) {
	var calls int
	pipe := newMinimaxToolCallPipe(func(chunk StreamChunk) error {
		calls++
		return nil
	}, &atomic.Int64{})

	if err := pipe.flush(); err != nil {
		t.Fatalf("flush empty: %v", err)
	}
	if calls != 0 {
		t.Errorf("expected 0 callbacks on empty flush, got %d", calls)
	}
}

func TestMiniMaxPipe_FlushEmitsBufferedText(t *testing.T) {
	var texts []string
	pipe := newMinimaxToolCallPipe(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		return nil
	}, &atomic.Int64{})

	// Buffer some text via handle
	_ = pipe.handle(StreamChunk{Type: "delta", TextDelta: "hello "})
	_ = pipe.handle(StreamChunk{Type: "delta", TextDelta: "world"})

	if err := pipe.flush(); err != nil {
		t.Fatalf("flush: %v", err)
	}

	// The handle calls tryParse which flushes when buffer > 256 bytes
	// Our buffer is small so nothing emitted until explicit flush
	if len(texts) != 1 {
		t.Errorf("expected 1 text after flush, got %d: %v", len(texts), texts)
	}
	if texts[0] != "hello world" {
		t.Errorf("expected 'hello world', got '%s'", texts[0])
	}
}

func TestMiniMaxPipe_StopFlushes(t *testing.T) {
	var texts []string
	pipe := newMinimaxToolCallPipe(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		return nil
	}, &atomic.Int64{})

	_ = pipe.handle(StreamChunk{Type: "delta", TextDelta: "hello"})
	_ = pipe.handle(StreamChunk{Type: "stop", FinishReason: "stop"})

	if len(texts) != 1 {
		t.Errorf("expected 1 text after stop, got %d", len(texts))
	}
}

func TestMiniMaxPipe_ThinkingFlushes(t *testing.T) {
	var texts []string
	var thinkTexts []string
	pipe := newMinimaxToolCallPipe(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		if chunk.Thinking != "" {
			thinkTexts = append(thinkTexts, chunk.Thinking)
		}
		return nil
	}, &atomic.Int64{})

	_ = pipe.handle(StreamChunk{Type: "delta", TextDelta: "some text"})
	_ = pipe.handle(StreamChunk{Type: "delta", Thinking: "thinking content"})

	if len(texts) != 1 {
		t.Errorf("expected 1 text after thinking flush, got %d: %v", len(texts), texts)
	}
	if len(thinkTexts) != 1 {
		t.Errorf("expected 1 thinking, got %d", len(thinkTexts))
	}
}

func TestMiniMaxPipe_NonTextDeltaPassedThrough(t *testing.T) {
	var chunks []StreamChunk
	pipe := newMinimaxToolCallPipe(func(chunk StreamChunk) error {
		chunks = append(chunks, chunk)
		return nil
	}, &atomic.Int64{})

	_ = pipe.handle(StreamChunk{Type: "delta", ToolCallID: "call_1", ToolCallName: "bash", JSONDelta: `{"cmd":"ls"}`})
	_ = pipe.handle(StreamChunk{Type: "complete"})

	if len(chunks) != 2 {
		t.Errorf("expected 2 chunks passed through, got %d", len(chunks))
	}
}

// --- tryParse with XML tool calls ---

func TestMiniMaxPipe_TryParseSimpleToolCall(t *testing.T) {
	var toolCalls []StreamChunk
	pipe := newMinimaxToolCallPipe(func(chunk StreamChunk) error {
		if chunk.ToolCallID != "" {
			toolCalls = append(toolCalls, chunk)
		}
		return nil
	}, &atomic.Int64{})

	// Feed a complete XML tool call block
	err := pipe.handle(StreamChunk{Type: "delta", TextDelta: `<minimax:tool_call><invoke name="bash"><parameter name="cmd">ls</parameter></invoke></minimax:tool_call>`})
	if err != nil {
		t.Fatalf("handle: %v", err)
	}

	if len(toolCalls) != 1 {
		t.Fatalf("expected 1 tool call, got %d", len(toolCalls))
	}
	if toolCalls[0].ToolCallName != "bash" {
		t.Errorf("expected ToolCallName 'bash', got '%s'", toolCalls[0].ToolCallName)
	}
}

func TestMiniMaxPipe_TryParseTextAndToolCall(t *testing.T) {
	var texts []string
	var toolCalls []StreamChunk
	pipe := newMinimaxToolCallPipe(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		if chunk.ToolCallID != "" {
			toolCalls = append(toolCalls, chunk)
		}
		return nil
	}, &atomic.Int64{})

	err := pipe.handle(StreamChunk{Type: "delta", TextDelta: `Let me run: <minimax:tool_call><invoke name="bash"><parameter name="cmd">ls</parameter></invoke></minimax:tool_call>`})
	if err != nil {
		t.Fatalf("handle: %v", err)
	}

	if len(texts) != 1 {
		t.Errorf("expected 1 text, got %d", len(texts))
	}
	if len(toolCalls) != 1 {
		t.Fatalf("expected 1 tool call, got %d", len(toolCalls))
	}
	if toolCalls[0].ToolCallName != "bash" {
		t.Errorf("expected ToolCallName 'bash', got '%s'", toolCalls[0].ToolCallName)
	}
}

func TestMiniMaxPipe_TryParseMultipleInvokes(t *testing.T) {
	var toolCalls []string
	pipe := newMinimaxToolCallPipe(func(chunk StreamChunk) error {
		if chunk.ToolCallName != "" {
			toolCalls = append(toolCalls, chunk.ToolCallName)
		}
		return nil
	}, &atomic.Int64{})

	err := pipe.handle(StreamChunk{Type: "delta", TextDelta: `<minimax:tool_call><invoke name="bash"><parameter name="cmd">ls</parameter></invoke><invoke name="read"><parameter name="path">/etc</parameter></invoke></minimax:tool_call>`})
	if err != nil {
		t.Fatalf("handle: %v", err)
	}

	if len(toolCalls) != 2 {
		t.Fatalf("expected 2 tool calls, got %d", len(toolCalls))
	}
	if toolCalls[0] != "bash" || toolCalls[1] != "read" {
		t.Errorf("expected [bash read], got %v", toolCalls)
	}
}

// --- extractToolCallsFromResult ---

func TestExtractToolCallsFromResult_NoToolCalls(t *testing.T) {
	result := &SendResult{
		Content: []ContentBlock{
			{Type: "text", Text: "hello world"},
		},
		Raw: []byte(`{}`),
	}
	var counter atomic.Int64
	out := extractToolCallsFromResult(result, &counter)
	if len(out.Content) != 1 {
		t.Errorf("expected 1 block, got %d", len(out.Content))
	}
	if out.Content[0].Type != "text" {
		t.Errorf("expected text block, got %s", out.Content[0].Type)
	}
}

func TestExtractToolCallsFromResult_WithToolCall(t *testing.T) {
	result := &SendResult{
		Content: []ContentBlock{
			{Type: "text", Text: `pre <minimax:tool_call><invoke name="bash"><parameter name="cmd">ls</parameter></invoke></minimax:tool_call> post`},
		},
		Raw: []byte(`{}`),
	}
	var counter atomic.Int64
	out := extractToolCallsFromResult(result, &counter)
	// Should split into: text("pre "), tool_use, text(" post")
	if len(out.Content) != 3 {
		t.Fatalf("expected 3 blocks, got %d", len(out.Content))
	}
	if out.Content[0].Type != "text" {
		t.Errorf("expected first block type 'text', got '%s'", out.Content[0].Type)
	}
	if out.Content[1].Type != "tool_use" {
		t.Errorf("expected second block type 'tool_use', got '%s'", out.Content[1].Type)
	}
	if out.Content[2].Type != "text" || out.Content[2].Text != " post" {
		t.Errorf("expected third block text ' post', got '%s'", out.Content[2].Text)
	}
}

// --- extractMiniMaxMetadata ---

func TestExtractMiniMaxMetadata_NilResult(t *testing.T) {
	result := extractMiniMaxMetadata(nil)
	if result != nil {
		t.Error("expected nil for nil input")
	}
}

func TestExtractMiniMaxMetadata_NilRaw(t *testing.T) {
	result := extractMiniMaxMetadata(&SendResult{Content: nil, Raw: nil})
	if result == nil {
		t.Error("expected non-nil result")
	}
}
