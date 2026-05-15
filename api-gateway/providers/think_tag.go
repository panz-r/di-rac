package providers

import "strings"

// ThinkTagConfig configures think-tag extraction behavior.
// Zero value uses defaults (<think>, </think>), suitable for most providers.
type ThinkTagConfig struct {
	// OpeningTag is the tag prefix for the opening tag (e.g. "<think", "<thinking").
	// Default: "<think"
	OpeningTag string
	// ClosingTag is the tag prefix for the closing tag (e.g. "</think", "</thinking").
	// Default: "</think"
	ClosingTag string
}

func (c *ThinkTagConfig) openingTag() string {
	if c.OpeningTag != "" {
		return c.OpeningTag
	}
	return "<think"
}

func (c *ThinkTagConfig) closingTag() string {
	if c.ClosingTag != "" {
		return c.ClosingTag
	}
	return "</think"
}

// SplitOnTag splits text at the first occurrence of tagPrefix and returns
// the part before and the part after the tag (with the tag stripped).
// If the tag is not found, returns the original text as "before" and "" as "rest".
func SplitOnTag(text, tagPrefix string) (before string, rest string) {
	idx := strings.Index(text, tagPrefix)
	if idx < 0 {
		return text, ""
	}
	before = text[:idx]
	rest = text[idx+len(tagPrefix):]
	// Strip closing > if present
	if strings.HasPrefix(rest, ">") {
		rest = rest[1:]
	}
	return before, rest
}

// NewThinkTagStream wraps a Stream callback to intercept <think>...</think> tags
// in TextDelta chunks and emit them as Thinking deltas instead.
// This is needed for providers where the model emits thinking content inside
// <think> tags in the text content rather than in a dedicated reasoning_content field.
// Providers that need this can opt in by wrapping their inner stream callback:
//
//	func (h *MyHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
//	    return h.inner.Stream(ctx, req, NewThinkTagStream(callback))
//	}
//
// Use NewThinkTagStreamConfig for non-default tags (e.g. "<thinking", "</thinking").
func NewThinkTagStream(callback func(StreamChunk) error) func(StreamChunk) error {
	return NewThinkTagStreamConfig(callback, ThinkTagConfig{})
}

// NewThinkTagStreamConfig is like NewThinkTagStream but with a configurable tag prefix.
func NewThinkTagStreamConfig(callback func(StreamChunk) error, cfg ThinkTagConfig) func(StreamChunk) error {
	openTag := cfg.openingTag()
	closeTag := cfg.closingTag()
	var inThinkBlock bool
	return func(chunk StreamChunk) error {
		if chunk.Type == "delta" && chunk.TextDelta != "" && chunk.Thinking == "" {
			content := chunk.TextDelta
			if inThinkBlock {
				before, rest := SplitOnTag(content, closeTag)
				if rest != "" || before != content {
					inThinkBlock = false
					before = strings.TrimSpace(before)
					rest = strings.TrimSpace(rest)
					if before != "" {
						if err := callback(StreamChunk{Type: "delta", Thinking: before}); err != nil {
							return err
						}
					}
					if rest != "" {
						return callback(StreamChunk{Type: "delta", TextDelta: rest})
					}
					return nil
				}
				return callback(StreamChunk{Type: "delta", Thinking: content})
			}
			before, rest := SplitOnTag(content, openTag)
			if rest != "" || before != content {
				inThinkBlock = true
				before = strings.TrimSpace(before)
				rest = strings.TrimSpace(rest)
				if before != "" {
					if err := callback(StreamChunk{Type: "delta", TextDelta: before}); err != nil {
						return err
					}
				}
				if rest != "" {
					// Check if the rest also contains the closing tag (same-chunk pair)
					closeBefore, closeRest := SplitOnTag(rest, closeTag)
					if closeRest != "" || closeBefore != rest {
						inThinkBlock = false
						closeBefore = strings.TrimSpace(closeBefore)
						closeRest = strings.TrimSpace(closeRest)
						if closeBefore != "" {
							if err := callback(StreamChunk{Type: "delta", Thinking: closeBefore}); err != nil {
								return err
							}
						}
						if closeRest != "" {
							return callback(StreamChunk{Type: "delta", TextDelta: closeRest})
						}
						return nil
					}
					return callback(StreamChunk{Type: "delta", Thinking: rest})
				}
				return nil
			}
		}
		if chunk.Type == "delta" && chunk.Thinking != "" {
			if strings.Contains(chunk.Thinking, closeTag) {
				inThinkBlock = false
			} else if strings.Contains(chunk.Thinking, openTag) {
				inThinkBlock = true
			}
		}
		return callback(chunk)
	}
}
