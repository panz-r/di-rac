package providers

import "testing"

func TestSplitOnTag_NotFound(t *testing.T) {
	before, rest := SplitOnTag("hello world", "<think")
	if before != "hello world" || rest != "" {
		t.Errorf(`expected ("hello world", ""), got (%q, %q)`, before, rest)
	}
}

func TestSplitOnTag_AtStart(t *testing.T) {
	before, rest := SplitOnTag("<think>deep thoughts", "<think")
	if before != "" || rest != "deep thoughts" {
		t.Errorf(`expected ("", "deep thoughts"), got (%q, %q)`, before, rest)
	}
}

func TestSplitOnTag_AtEnd(t *testing.T) {
	before, rest := SplitOnTag("some text</think", "</think")
	if before != "some text" || rest != "" {
		t.Errorf(`expected ("some text", ""), got (%q, %q)`, before, rest)
	}
}

func TestSplitOnTag_StripsGt(t *testing.T) {
	before, rest := SplitOnTag("<think>content", "<think")
	if before != "" || rest != "content" {
		t.Errorf(`expected ("", "content"), got (%q, %q)`, before, rest)
	}
}

func TestSplitOnTag_Middle(t *testing.T) {
	before, rest := SplitOnTag("lead<think>trail", "<think")
	if before != "lead" || rest != "trail" {
		t.Errorf(`expected ("lead", "trail"), got (%q, %q)`, before, rest)
	}
}

func TestSplitOnTag_FirstOnly(t *testing.T) {
	before, rest := SplitOnTag("a<think>b<think>c", "<think")
	if before != "a" || rest != "b<think>c" {
		t.Errorf(`expected ("a", "b<think>c"), got (%q, %q)`, before, rest)
	}
}

func TestThinkTagStream_Passthrough(t *testing.T) {
	var result []StreamChunk
	wrapped := NewThinkTagStream(func(chunk StreamChunk) error {
		result = append(result, chunk)
		return nil
	})

	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "hello world"})
	_ = wrapped(StreamChunk{Type: "stop", FinishReason: "stop"})

	if len(result) != 2 {
		t.Fatalf("expected 2 chunks, got %d", len(result))
	}
	if result[0].TextDelta != "hello world" {
		t.Errorf("expected TextDelta 'hello world', got %q", result[0].TextDelta)
	}
	if result[1].FinishReason != "stop" {
		t.Errorf("expected FinishReason 'stop', got %q", result[1].FinishReason)
	}
}

func TestThinkTagStream_OpeningTag(t *testing.T) {
	var texts []string
	var thinks []string
	wrapped := NewThinkTagStream(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		if chunk.Thinking != "" {
			thinks = append(thinks, chunk.Thinking)
		}
		return nil
	})

	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "before<think>during"})

	if len(texts) != 1 || texts[0] != "before" {
		t.Errorf("expected TextDelta ['before'], got %v", texts)
	}
	if len(thinks) != 1 || thinks[0] != "during" {
		t.Errorf("expected Thinking ['during'], got %v", thinks)
	}
}

func TestThinkTagStream_ClosingTag(t *testing.T) {
	var texts []string
	var thinks []string
	wrapped := NewThinkTagStream(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		if chunk.Thinking != "" {
			thinks = append(thinks, chunk.Thinking)
		}
		return nil
	})

	// First enter think block, then close it in a separate chunk
	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "<think>thinking text"})
	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "</think>after"})

	if len(thinks) != 1 || thinks[0] != "thinking text" {
		t.Errorf("expected Thinking ['thinking text'], got %v", thinks)
	}
	if len(texts) != 1 || texts[0] != "after" {
		t.Errorf("expected TextDelta ['after'], got %v", texts)
	}
}

func TestThinkTagStream_MultiChunk(t *testing.T) {
	var texts []string
	var thinks []string
	wrapped := NewThinkTagStream(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		if chunk.Thinking != "" {
			thinks = append(thinks, chunk.Thinking)
		}
		return nil
	})

	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "before<think"})
	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "deep thoughts"})
	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "</think>after"})

	if len(texts) != 2 || texts[0] != "before" || texts[1] != "after" {
		t.Errorf("expected TextDelta ['before', 'after'], got %v", texts)
	}
	if len(thinks) != 1 || thinks[0] != "deep thoughts" {
		t.Errorf("expected Thinking ['deep thoughts'], got %v", thinks)
	}
}

func TestThinkTagStream_OpeningTagAtEndOfChunk(t *testing.T) {
	var texts []string
	var thinks []string
	wrapped := NewThinkTagStream(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		if chunk.Thinking != "" {
			thinks = append(thinks, chunk.Thinking)
		}
		return nil
	})

	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "hello<think"})
	_ = wrapped(StreamChunk{Type: "delta", TextDelta: ">thoughtful"})

	if len(texts) != 1 || texts[0] != "hello" {
		t.Errorf("expected TextDelta ['hello'], got %v", texts)
	}
	if len(thinks) != 1 || thinks[0] != ">thoughtful" {
		t.Errorf("expected Thinking ['>thoughtful'], got %v", thinks)
	}
}

func TestThinkTagStream_ConfigCustomTags(t *testing.T) {
	var texts []string
	var thinks []string
	wrapped := NewThinkTagStreamConfig(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		if chunk.Thinking != "" {
			thinks = append(thinks, chunk.Thinking)
		}
		return nil
	}, ThinkTagConfig{OpeningTag: "<thinking", ClosingTag: "</thinking"})

	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "lead<thinking>think"})

	if len(texts) != 1 || texts[0] != "lead" {
		t.Errorf("expected TextDelta ['lead'], got %v", texts)
	}
	if len(thinks) != 1 || thinks[0] != "think" {
		t.Errorf("expected Thinking ['think'], got %v", thinks)
	}
}

func TestThinkTagStream_OpeningAndClosingInSameChunk(t *testing.T) {
	var texts []string
	var thinks []string
	wrapped := NewThinkTagStream(func(chunk StreamChunk) error {
		if chunk.TextDelta != "" {
			texts = append(texts, chunk.TextDelta)
		}
		if chunk.Thinking != "" {
			thinks = append(thinks, chunk.Thinking)
		}
		return nil
	})

	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "before<think>during</think>after"})

	if len(texts) != 2 || texts[0] != "before" || texts[1] != "after" {
		t.Errorf("expected TextDelta ['before', 'after'], got %v", texts)
	}
	if len(thinks) != 1 || thinks[0] != "during" {
		t.Errorf("expected Thinking ['during'], got %v", thinks)
	}
}

func TestThinkTagStream_SameChunkPairStateCleared(t *testing.T) {
	var thinks []string
	wrapped := NewThinkTagStream(func(chunk StreamChunk) error {
		if chunk.Thinking != "" {
			thinks = append(thinks, chunk.Thinking)
		}
		return nil
	})

	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "<think>think</think>"})
	_ = wrapped(StreamChunk{Type: "delta", TextDelta: "normal text"})

	if len(thinks) != 1 {
		t.Errorf("expected 1 thinking chunk, got %d", len(thinks))
	}
}
