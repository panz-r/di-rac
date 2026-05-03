package providers

import (
	"context"
	"encoding/json"
	"log"
	"regexp"
	"strings"
)

// MiniMaxHandler translates between our internal protocol and MiniMax's native API.
//
// MiniMax-M2 models output tool calls as <minimax:tool_call> XML in text content.
// This handler:
//   - Sends requests to MiniMax's native endpoint (https://api.minimax.io/v1/chat/completions)
//   - Tool definitions use MiniMax's native format: {type: "function", function: {name, description, parameters}}
//   - Parses <minimax:tool_call> XML from text responses into structured StreamChunk tool calls
//
// The HTTP/SSE transport is handled by the shared openaiCompatHandler (pure transport,
// no format translation). Format translation happens in the MiniMax-specific layers below.
type MiniMaxHandler struct {
	inner *openaiCompatHandler
}

func NewMiniMaxHandler() *MiniMaxHandler {
	temp := 1.0
	return &MiniMaxHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.minimax.io/v1",
			DefaultModel: "MiniMax-M2.7",
			Temperature:  &temp,
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				result["reasoning_split"] = true
				// Log request summary for debugging
				model, _ := result["model"].(string)
				nTools := 0
				if tools, ok := result["tools"].([]map[string]interface{}); ok {
					nTools = len(tools)
				}
				log.Printf("[MiniMax] request: model=%s tools=%d stream=%v", model, nTools, result["stream"])
			},
		}),
	}
}

func (h *MiniMaxHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	result, err := h.inner.Send(ctx, req)
	if err != nil {
		return nil, err
	}
	return extractToolCallsFromResult(result), nil
}

func (h *MiniMaxHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	pipe := newMinimaxToolCallPipe(callback)
	err := h.inner.Stream(ctx, req, pipe.handle)
	pipe.flush()
	log.Printf("[MiniMax] stream complete: buffered=%d xmlParsed=%d", pipe.totalBuffered, pipe.totalXmlParsed)
	return err
}

var _ Handler = (*MiniMaxHandler)(nil)

// ---------------------------------------------------------------------------
// Streaming pipe: buffers text deltas, extracts <minimax:tool_call> XML blocks,
// and emits structured tool call StreamChunks.
// ---------------------------------------------------------------------------

type minimaxToolCallPipe struct {
	callback        func(StreamChunk) error
	textBuffer      strings.Builder
	totalBuffered   int
	totalXmlParsed  int
}

func newMinimaxToolCallPipe(callback func(StreamChunk) error) *minimaxToolCallPipe {
	return &minimaxToolCallPipe{callback: callback}
}

var (
	reToolCallOpen  = regexp.MustCompile(`<minimax:tool_call>`)
	reToolCallClose = regexp.MustCompile(`</minimax:tool_call>`)
	reInvoke        = regexp.MustCompile(`<invoke\s+name="([^"]*)">(.*?)</invoke>`)
	reParam         = regexp.MustCompile(`<parameter\s+name="([^"]*)">(.*?)</parameter>`)
)

func (p *minimaxToolCallPipe) handle(chunk StreamChunk) error {
	// Only intercept text deltas
	if chunk.Type == "delta" && chunk.TextDelta != "" {
		p.textBuffer.WriteString(chunk.TextDelta)
		p.totalBuffered++
		// Log text content to see if MiniMax sends XML tool calls or plain text
		if strings.Contains(chunk.TextDelta, "<minimax") || strings.Contains(chunk.TextDelta, "<invoke") {
			log.Printf("[MiniMax] XML fragment in text delta: %q", chunk.TextDelta)
		} else if p.totalBuffered <= 3 || p.textBuffer.Len() > 64 {
			// Log first few chunks and when buffer grows, to see what model outputs
			log.Printf("[MiniMax] text delta #%d (buf=%d): %q", p.totalBuffered, p.textBuffer.Len(), truncate(chunk.TextDelta, 200))
		}
		return p.tryParse()
	}

	// Log non-text deltas (tool calls from SSE, thinking, etc.)
	if chunk.Type == "delta" && (chunk.JSONDelta != "" || chunk.ToolCallID != "" || chunk.ToolCallName != "") {
		log.Printf("[MiniMax] structured delta: id=%s name=%s json_len=%d", chunk.ToolCallID, chunk.ToolCallName, len(chunk.JSONDelta))
	}

	// For stop/complete chunks, flush any remaining buffered text first
	if chunk.Type == "stop" || chunk.Type == "complete" {
		if p.textBuffer.Len() > 0 {
			log.Printf("[MiniMax] flushing %d bytes on %s: %q", p.textBuffer.Len(), chunk.Type, truncate(p.textBuffer.String(), 300))
		}
		p.flush()
	}

	return p.callback(chunk)
}

func truncate(s string, maxLen int) string {
	if len(s) <= maxLen {
		return s
	}
	return s[:maxLen] + "..."
}

func (p *minimaxToolCallPipe) tryParse() error {
	buf := p.textBuffer.String()

	// Only parse once we see the closing tag (complete tool call block)
	if !strings.Contains(buf, "</minimax:tool_call>") {
		// If the buffer is getting large with no tool calls, flush as text
		if !strings.Contains(buf, "<minimax:tool_call>") && len(buf) > 256 {
			p.textBuffer.Reset()
			return p.callback(StreamChunk{Type: "delta", TextDelta: buf})
		}
		return nil
	}

	p.textBuffer.Reset()

	// Split into segments: text before tool calls, tool call blocks, text between/after
	segments := reToolCallOpen.Split(buf, -1)
	for i, seg := range segments {
		if i == 0 && seg != "" {
			// Text before first tool call
			if err := p.callback(StreamChunk{Type: "delta", TextDelta: seg}); err != nil {
				return err
			}
			continue
		}
		if i == 0 {
			continue
		}

		// Split on closing tag
		parts := reToolCallClose.Split(seg, 2)
		xmlBlock := parts[0]

		// Parse invokes from the XML block
		invokeMatches := reInvoke.FindAllStringSubmatch(xmlBlock, -1)
		p.totalXmlParsed += len(invokeMatches)
		for _, inv := range invokeMatches {
			toolName := inv[1]
			paramsXML := inv[2]

			// Parse parameters
			args := map[string]interface{}{}
			paramMatches := reParam.FindAllStringSubmatch(paramsXML, -1)
			for _, pm := range paramMatches {
				paramName := pm[1]
				paramVal := strings.TrimSpace(pm[2])
				// Try JSON parse for objects/arrays, keep as string otherwise
				var parsed interface{}
				if err := json.Unmarshal([]byte(paramVal), &parsed); err == nil {
					args[paramName] = parsed
				} else {
					args[paramName] = paramVal
				}
			}

			argsJSON, _ := json.Marshal(args)

			// Emit tool call delta
			if err := p.callback(StreamChunk{
				Type:         "delta",
				Index:        0,
				ToolCallID:   "minimax_" + toolName,
				ToolCallName: toolName,
				JSONDelta:    string(argsJSON),
			}); err != nil {
				return err
			}
		}

		// Text after the closing tag
		if len(parts) > 1 && parts[1] != "" {
			// If there's another tool call opening, don't emit yet
			if !strings.Contains(parts[1], "<minimax:tool_call>") {
				if err := p.callback(StreamChunk{Type: "delta", TextDelta: parts[1]}); err != nil {
					return err
				}
			} else {
				p.textBuffer.WriteString(parts[1])
			}
		}
	}

	return nil
}

func (p *minimaxToolCallPipe) flush() {
	if p.textBuffer.Len() > 0 {
		text := p.textBuffer.String()
		p.textBuffer.Reset()
		p.callback(StreamChunk{Type: "delta", TextDelta: text})
	}
}

// ---------------------------------------------------------------------------
// Non-streaming: extract tool calls from text content blocks in result
// ---------------------------------------------------------------------------

func extractToolCallsFromResult(result *SendResult) *SendResult {
	if result == nil {
		return result
	}
	var newBlocks []ContentBlock
	for _, block := range result.Content {
		if block.Type != "text" || !strings.Contains(block.Text, "<minimax:tool_call>") {
			newBlocks = append(newBlocks, block)
			continue
		}
		// Split text on <minimax:tool_call> blocks
		segments := reToolCallOpen.Split(block.Text, -1)
		for i, seg := range segments {
			if i == 0 {
				if seg != "" {
					newBlocks = append(newBlocks, ContentBlock{Type: "text", Text: seg})
				}
				continue
			}
			parts := reToolCallClose.Split(seg, 2)
			xmlBlock := parts[0]

			invokeMatches := reInvoke.FindAllStringSubmatch(xmlBlock, -1)
			for _, inv := range invokeMatches {
				toolName := inv[1]
				paramsXML := inv[2]
				args := map[string]interface{}{}
				paramMatches := reParam.FindAllStringSubmatch(paramsXML, -1)
				for _, pm := range paramMatches {
					paramName := pm[1]
					paramVal := strings.TrimSpace(pm[2])
					var parsed interface{}
					if err := json.Unmarshal([]byte(paramVal), &parsed); err == nil {
						args[paramName] = parsed
					} else {
						args[paramName] = paramVal
					}
				}
				argsJSON, _ := json.Marshal(args)
				newBlocks = append(newBlocks, ContentBlock{
					Type: "tool_use",
					ToolUse: &ToolUseBlock{
						ID:   "minimax_" + toolName,
						Type: "tool_use",
						Function: struct {
							Name      string `json:"name"`
							Arguments string `json:"arguments"`
						}{Name: toolName, Arguments: string(argsJSON)},
					},
				})
			}

			// Text after closing tag
			if len(parts) > 1 && parts[1] != "" {
				newBlocks = append(newBlocks, ContentBlock{Type: "text", Text: parts[1]})
			}
		}
	}
	result.Content = newBlocks
	return result
}
