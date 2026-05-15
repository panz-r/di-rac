package providers

import (
	"context"
	"encoding/json"
	"encoding/xml"
	"fmt"
	"log"
	"strings"
	"sync/atomic"
	"time"
)

// MiniMax-specific response metadata
type MiniMaxMetadata struct {
	ReasoningDetails *ReasoningDetails `json:"reasoning_details,omitempty"`
	CacheInfo        *CacheInfo        `json:"cache_info,omitempty"`
}

type ReasoningDetails struct {
	Text   string `json:"reasoning_text,omitempty"`
	Tokens int    `json:"reasoning_tokens,omitempty"`
}

type CacheInfo struct {
	CacheHitTokens int `json:"cache_hit_tokens,omitempty"`
	NewInputTokens  int `json:"new_input_tokens,omitempty"`
	OutputTokens    int `json:"output_tokens,omitempty"`
}

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
	inner       *openaiCompatHandler
	callCounter atomic.Int64
}

func NewMiniMaxHandler() *MiniMaxHandler {
	const defaultModel = "MiniMax-M2.7"
	temp := 1.0
	return &MiniMaxHandler{
		inner: newOpenAICompatHandler(OpenAICompatConfig{
			BaseURL:      "https://api.minimax.io/v1",
			DefaultModel: defaultModel,
			Temperature:  &temp,
			Capabilities: &ProviderInfo{
				ID:           "minimax",
				DefaultModel: defaultModel,
				MaxTokensDefault: 16384,
				Features: ProviderFeatures{
					SupportsThinking:    false,
					SupportsTools:       true,
					SupportsImages:      false,
					SupportsPromptCache: false,
					SupportsStreaming:   true,
				},
				Settings: []ProviderSetting{
					{
						Key:         "temperature",
						Label:       "Temperature",
						Type:        SettingSlider,
						Min:         fPtr(0),
						Max:         fPtr(1),
						Step:        fPtr(0.1),
						Default:     1.0,
						Group:       "sampling",
						Description: "Controls randomness (0 = deterministic, 1 = creative).",
						ValidRange:  "0 – 1",
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
			},
			ModifyRequest: func(req *Request, result map[string]interface{}) {
				result["reasoning_split"] = true
				model, _ := result["model"].(string)
				nTools := 0
				if tools, ok := result["tools"].([]map[string]interface{}); ok {
					nTools = len(tools)
				}
				msgs, _ := result["messages"].([]map[string]interface{})
				nToolResults := 0
				for _, m := range msgs {
					if m["role"] == "tool" {
						nToolResults++
					}
				}
				log.Printf("[MiniMax] request: model=%s tools=%d msgs=%d toolResults=%d stream=%v", model, nTools, len(msgs), nToolResults, result["stream"])
			},
		}),
	}
}
func (h *MiniMaxHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	const maxRetries = 3
	var lastErr error

	for attempt := 0; attempt < maxRetries; attempt++ {
		result, err := h.inner.Send(ctx, req)
		if err != nil {
			if isRateLimitError(err) {
				waitTime := time.Duration(1<<attempt) * time.Second
				log.Printf("[MiniMax] Rate limited. Retry %d/%d in %v", attempt+1, maxRetries, waitTime)
				select {
				case <-time.After(waitTime):
					lastErr = err
					continue
				case <-ctx.Done():
					return nil, ctx.Err()
				}
			}
			return nil, err
		}
		result = extractMiniMaxMetadata(result)
		return extractToolCallsFromResult(result, &h.callCounter), nil
	}

	return nil, fmt.Errorf("[MiniMax] max retries exceeded: %v", lastErr)
}

func isRateLimitError(err error) bool {
	if err == nil {
		return false
	}
	return strings.Contains(err.Error(), "429") || strings.Contains(err.Error(), "rate limit")
}

func extractMiniMaxMetadata(result *SendResult) *SendResult {
	if result == nil || result.Raw == nil {
		return result
	}

	var rawResp map[string]interface{}
	if err := json.Unmarshal(result.Raw, &rawResp); err != nil {
		log.Printf("[MiniMax] failed to parse raw response: %v", err)
		return result
	}

	if reasoning, ok := rawResp["reasoning_details"].(map[string]interface{}); ok {
		if tokens, ok := reasoning["reasoning_tokens"].(float64); ok {
			log.Printf("[MiniMax] reasoning_tokens: %d", int(tokens))
		}
	}

	if cacheInfo, ok := rawResp["cache_info"].(map[string]interface{}); ok {
		if hitTokens, ok := cacheInfo["cache_hit_tokens"].(float64); ok {
			log.Printf("[MiniMax] cache_hit_tokens: %d", int(hitTokens))
		}
		if newTokens, ok := cacheInfo["new_input_tokens"].(float64); ok {
			log.Printf("[MiniMax] new_input_tokens: %d", int(newTokens))
		}
		if outTokens, ok := cacheInfo["output_tokens"].(float64); ok {
			log.Printf("[MiniMax] output_tokens: %d", int(outTokens))
		}
	}

	return result
}


func (h *MiniMaxHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	pipe := newMinimaxToolCallPipe(callback, &h.callCounter)
	err := h.inner.Stream(ctx, req, pipe.handle)
	if flushErr := pipe.flush(); flushErr != nil && err == nil {
		err = flushErr
	}
	log.Printf("[MiniMax] stream complete: buffered=%d xmlParsed=%d", pipe.totalBuffered, pipe.totalXmlParsed)
	return err
}

func (h *MiniMaxHandler) Capabilities() *ProviderInfo {
	return h.inner.Capabilities()
}

var _ Handler = (*MiniMaxHandler)(nil)

// XML structs for parsing <minimax:tool_call> blocks
type minimaxToolCallBlock struct {
	XMLName xml.Name         `xml:"tool_call"`
	Invokes []minimaxInvoke  `xml:"invoke"`
}

type minimaxInvoke struct {
	XMLName    xml.Name        `xml:"invoke"`
	Name       string          `xml:"name,attr"`
	Parameters []minimaxParam  `xml:"parameter"`
}

type minimaxParam struct {
	XMLName xml.Name `xml:"parameter"`
	Name    string   `xml:"name,attr"`
	Value   string   `xml:",innerxml"`
}

// ---------------------------------------------------------------------------
// Streaming pipe: buffers text deltas, extracts <minimax:tool_call> XML blocks,
// and emits structured tool call StreamChunks.
// ---------------------------------------------------------------------------

type minimaxToolCallPipe struct {
	callback       func(StreamChunk) error
	textBuffer     strings.Builder
	totalBuffered  int
	totalXmlParsed int
	counter        *atomic.Int64
}

const minimaxMaxBuffer = 1 << 20 // 1 MB cap on text buffer

func newMinimaxToolCallPipe(callback func(StreamChunk) error, counter *atomic.Int64) *minimaxToolCallPipe {
	return &minimaxToolCallPipe{callback: callback, counter: counter}
}


func (p *minimaxToolCallPipe) handle(chunk StreamChunk) error {
	// Only intercept text deltas
	if chunk.Type == "delta" && chunk.TextDelta != "" {
		p.textBuffer.WriteString(chunk.TextDelta)
		p.totalBuffered++
		// If buffer exceeds cap, flush as plain text to prevent unbounded growth.
		if p.textBuffer.Len() > minimaxMaxBuffer {
			flushed := p.textBuffer.String()
			p.textBuffer.Reset()
			if err := p.callback(StreamChunk{Type: "delta", TextDelta: flushed}); err != nil {
				return err
			}
		}
		// Log text content to see if MiniMax sends XML tool calls or plain text
		if strings.Contains(chunk.TextDelta, "<minimax") || strings.Contains(chunk.TextDelta, "<invoke") {
		} else if p.totalBuffered <= 3 || p.textBuffer.Len() > 64 {
			// Log first few chunks and when buffer grows, to see what model outputs
			log.Printf("[MiniMax] text delta: bytes=%d", len(chunk.TextDelta))
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
		if err := p.flush(); err != nil {
			return err
		}
	}

	// Models may emit the same tokens first as content (TextDelta, buffered
	// above) then as reasoning_details (Thinking). Without flushing here,
	// the buffered text would be emitted later, causing duplication.
	if chunk.Type == "delta" && chunk.Thinking != "" {
		if err := p.flush(); err != nil {
			return err
		}
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

	// Split into segments around <minimax:tool_call>...</minimax:tool_call> blocks
	openTag := "<minimax:tool_call>"
	closeTag := "</minimax:tool_call>"

	for {
		openIdx := strings.Index(buf, openTag)
		if openIdx == -1 {
			// No more tool calls — emit remaining text
			if buf != "" {
				if err := p.callback(StreamChunk{Type: "delta", TextDelta: buf}); err != nil {
					return err
				}
			}
			break
		}

		// Emit text before the opening tag
		if openIdx > 0 {
			if err := p.callback(StreamChunk{Type: "delta", TextDelta: buf[:openIdx]}); err != nil {
				return err
			}
		}

		closeIdx := strings.Index(buf, closeTag)
		if closeIdx == -1 {
			// Incomplete block — re-buffer from the opening tag
			p.textBuffer.WriteString(buf[openIdx:])
			break
		}

		// Extract the full XML block (including tags for unmarshaling)
		xmlBlock := buf[openIdx : closeIdx+len(closeTag)]
		buf = buf[closeIdx+len(closeTag):]

		// Parse the XML block
		var block minimaxToolCallBlock
		if err := xml.Unmarshal([]byte(xmlBlock), &block); err != nil {
			log.Printf("[MiniMax] XML parse error, emitting raw text: %v — %q", err, truncate(xmlBlock, 200))
			if err := p.callback(StreamChunk{Type: "delta", TextDelta: xmlBlock}); err != nil {
				return err
			}
			continue
		}

		p.totalXmlParsed += len(block.Invokes)
		for _, inv := range block.Invokes {
			args := map[string]interface{}{}
			for _, pm := range inv.Parameters {
				paramVal := strings.TrimSpace(pm.Value)
				args[pm.Name] = paramVal
			}
			argsJSON, _ := json.Marshal(args)
			callID := fmt.Sprintf("minimax_%d_%s", p.counter.Add(1), inv.Name)

			if err := p.callback(StreamChunk{
				Type:         "delta",
				Index:        0,
				ToolCallID:   callID,
				ToolCallName: inv.Name,
				JSONDelta:    string(argsJSON),
			}); err != nil {
				return err
			}
		}

		// If remaining buf has no more tool calls, buffer it for re-check or emit
		if !strings.Contains(buf, openTag) {
			if buf != "" {
				if err := p.callback(StreamChunk{Type: "delta", TextDelta: buf}); err != nil {
					return err
				}
			}
			break
		}
	}

	return nil
}

func (p *minimaxToolCallPipe) flush() error {
	if p.textBuffer.Len() > 0 {
		text := p.textBuffer.String()
		p.textBuffer.Reset()
		return p.callback(StreamChunk{Type: "delta", TextDelta: text})
	}
	return nil
}

// ---------------------------------------------------------------------------
// Non-streaming: extract tool calls from text content blocks in result
// ---------------------------------------------------------------------------

func extractToolCallsFromResult(result *SendResult, counter *atomic.Int64) *SendResult {
	if result == nil {
		return result
	}
	var newBlocks []ContentBlock
	openTag := "<minimax:tool_call>"
	closeTag := "</minimax:tool_call>"

	for _, block := range result.Content {
		if block.Type != "text" || !strings.Contains(block.Text, openTag) {
			newBlocks = append(newBlocks, block)
			continue
		}

		buf := block.Text
		for {
			openIdx := strings.Index(buf, openTag)
			if openIdx == -1 {
				if buf != "" {
					newBlocks = append(newBlocks, ContentBlock{Type: "text", Text: buf})
				}
				break
			}

			if openIdx > 0 {
				newBlocks = append(newBlocks, ContentBlock{Type: "text", Text: buf[:openIdx]})
			}

			closeIdx := strings.Index(buf, closeTag)
			if closeIdx == -1 {
				log.Printf("[MiniMax] incomplete XML block in non-streaming result: %q", truncate(buf[openIdx:], 200))
				newBlocks = append(newBlocks, ContentBlock{Type: "text", Text: buf[openIdx:]})
				break
			}

			xmlBlock := buf[openIdx : closeIdx+len(closeTag)]
			buf = buf[closeIdx+len(closeTag):]

			var xmlBlockStruct minimaxToolCallBlock
			if err := xml.Unmarshal([]byte(xmlBlock), &xmlBlockStruct); err != nil {
				log.Printf("[MiniMax] XML parse error in non-streaming: %v — %q", err, truncate(xmlBlock, 200))
				newBlocks = append(newBlocks, ContentBlock{Type: "text", Text: xmlBlock})
				continue
			}

			for _, inv := range xmlBlockStruct.Invokes {
				args := map[string]interface{}{}
				for _, pm := range inv.Parameters {
					paramVal := strings.TrimSpace(pm.Value)
					if paramVal == "" {
						continue
					}
						args[pm.Name] = paramVal
				}
				argsJSON, _ := json.Marshal(args)
				callID := fmt.Sprintf("minimax_%d_%s", counter.Add(1), inv.Name)
				newBlocks = append(newBlocks, ContentBlock{
					Type: "tool_use",
					ToolUse: &ToolUseBlock{
						ID:   callID,
						Type: "tool_use",
						Function: struct {
							Name      string `json:"name"`
							Arguments string `json:"arguments"`
						}{Name: inv.Name, Arguments: string(argsJSON)},
					},
				})
			}
		}
	}
	result.Content = newBlocks
	return result
}

func (h *MiniMaxHandler) ListModels(ctx context.Context, cfg ProviderConfig) ([]ModelEntry, error) {
	return h.inner.ListModels(ctx, cfg)
}

func (h *MiniMaxHandler) ValidateSettings(settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	return BaseValidateSettings(h.Capabilities(), settings, thinking)
}

var _ ModelLister = (*MiniMaxHandler)(nil)
var _ CapableHandler = (*MiniMaxHandler)(nil)
var _ SettingsValidator = (*MiniMaxHandler)(nil)
