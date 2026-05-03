package providers

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"

	"github.com/google/generative-ai-go/genai"
	"google.golang.org/api/option"
)

// GeminiHandler handles Gemini API requests
type GeminiHandler struct {
	client *genai.Client
}

func NewGeminiHandler() *GeminiHandler {
	return &GeminiHandler{}
}

func NewGeminiHandlerWithKey(apiKey string) (*GeminiHandler, error) {
	client, err := genai.NewClient(context.Background(), option.WithAPIKey(apiKey))
	if err != nil {
		return nil, fmt.Errorf("failed to create Gemini client: %w", err)
	}
	return &GeminiHandler{client: client}, nil
}

func (h *GeminiHandler) getClient(req *Request) (*genai.Client, error) {
	if req.Provider.APIKey != "" {
		client, err := genai.NewClient(context.Background(), option.WithAPIKey(req.Provider.APIKey))
		if err != nil {
			return nil, fmt.Errorf("failed to create Gemini client: %w", err)
		}
		return client, nil
	}
	if h.client == nil {
		return nil, fmt.Errorf("Gemini client not configured (no API key)")
	}
	return h.client, nil
}

func (h *GeminiHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
	client, err := h.getClient(req)
	if err != nil {
		return nil, err
	}

	model := h.configureModel(client, req)
	session, lastParts := h.buildSession(model, req)

	iter := session.SendMessageStream(ctx, lastParts...)
	var contentBlocks []ContentBlock
	var usage *Usage
	var stopReason string

	for {
		resp, err := iter.Next()
		if err != nil {
			break
		}
		result := h.convertChunk(resp)
		if result.ContentBlocks != nil {
			contentBlocks = append(contentBlocks, result.ContentBlocks...)
		}
		if result.Usage != nil {
			usage = result.Usage
		}
		if result.FinishReason != "" {
			stopReason = result.FinishReason
		}
	}

	if usage == nil {
		usage = &Usage{}
	}
	return &SendResult{
		Content:    contentBlocks,
		Model:      req.Provider.Model,
		Usage:      usage,
		StopReason: stopReason,
	}, nil
}

func (h *GeminiHandler) Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error {
	client, err := h.getClient(req)
	if err != nil {
		return err
	}

	model := h.configureModel(client, req)
	session, lastParts := h.buildSession(model, req)

	iter := session.SendMessageStream(ctx, lastParts...)
	for {
		resp, err := iter.Next()
		if err != nil {
			break
		}
		chunk := h.convertChunk(resp)
		if err := callback(chunk); err != nil {
			return err
		}
	}

	return callback(StreamChunk{Type: "complete"})
}

func (h *GeminiHandler) configureModel(client *genai.Client, req *Request) *genai.GenerativeModel {
	modelName := req.Provider.Model
	if req.ModelOverride != "" {
		modelName = req.ModelOverride
	}
	if modelName == "" {
		modelName = "gemini-2.0-flash"
	}

	model := client.GenerativeModel(modelName)

	// System instruction
	if req.System != "" {
		model.SystemInstruction = &genai.Content{
			Parts: []genai.Part{genai.Text(req.System)},
		}
	}

	// Generation config
	if req.Temperature > 0 {
		temp := float32(req.Temperature)
		model.Temperature = &temp
	}
	if req.TopP > 0 {
		topP := float32(req.TopP)
		model.TopP = &topP
	}
	if req.MaxTokens > 0 {
		maxTokens := int32(req.MaxTokens)
		model.MaxOutputTokens = &maxTokens
	}
	if len(req.Stop) > 0 {
		model.StopSequences = req.Stop
	}

	// Tools
	if len(req.Tools) > 0 {
		var decls []*genai.FunctionDeclaration
		for _, toolJSON := range req.Tools {
			var tool struct {
				Name        string          `json:"name"`
				Description string          `json:"description"`
				InputSchema json.RawMessage `json:"input_schema"`
			}
			if err := json.Unmarshal(toolJSON, &tool); err != nil {
				continue
			}
			fd := &genai.FunctionDeclaration{
				Name:        tool.Name,
				Description: tool.Description,
			}
			if len(tool.InputSchema) > 0 {
				fd.Parameters = jsonSchemaToGeminiSchema(tool.InputSchema)
			}
			decls = append(decls, fd)
		}
		if len(decls) > 0 {
			model.Tools = []*genai.Tool{{FunctionDeclarations: decls}}
		}
	}

	return model
}

// buildSession creates a ChatSession with history from all messages except the last one,
// and returns the session along with the last user message's parts.
// This enables proper multi-turn conversation with the Gemini SDK.
func (h *GeminiHandler) buildSession(model *genai.GenerativeModel, req *Request) (*genai.ChatSession, []genai.Part) {
	// Pre-scan to build tool_use_id → function name map
	toolUseIDToName := map[string]string{}
	for _, msg := range req.Messages {
		for _, block := range msg.ContentBlocks {
			if block.Type == "tool_use" && block.ToolUse != nil {
				toolUseIDToName[block.ToolUse.ID] = block.ToolUse.Function.Name
			}
		}
	}

	session := model.StartChat()
	var lastParts []genai.Part

	for i, msg := range req.Messages {
		role := "user"
		if msg.Role == "assistant" {
			role = "model"
		}

		var parts []genai.Part
		if len(msg.ContentBlocks) > 0 {
			parts = h.convertContentBlocks(msg.ContentBlocks, toolUseIDToName)
		} else {
			// Legacy fallback
			if msg.Content != "" {
				parts = append(parts, genai.Text(msg.Content))
			}
			if msg.Thinking != "" {
				parts = append(parts, genai.Text(msg.Thinking))
			}
			if msg.ToolResult != nil {
				name := toolUseIDToName[msg.ToolResult.ToolUseID]
				parts = append(parts, genai.FunctionResponse{
					Name:     name,
					Response: map[string]any{"result": msg.ToolResult.Content},
				})
			}
		}

		if len(parts) == 0 {
			continue
		}

		// Last user message gets sent via SendMessage, rest goes into History
		isLast := i == len(req.Messages)-1 && msg.Role == "user"
		if isLast {
			lastParts = parts
		} else {
			session.History = append(session.History, &genai.Content{
				Role:  role,
				Parts: parts,
			})
		}
	}

	// If no last user message was found, just use empty text
	if len(lastParts) == 0 {
		lastParts = []genai.Part{genai.Text("")}
	}

	return session, lastParts
}

// buildContents converts messages to Gemini Content objects with proper roles.
// Used internally for non-chat scenarios.
func (h *GeminiHandler) buildContents(req *Request) []*genai.Content {
	// Pre-scan to build tool_use_id → function_name map (needed for FunctionResponse)
	toolUseIDToName := map[string]string{}
	for _, msg := range req.Messages {
		for _, block := range msg.ContentBlocks {
			if block.Type == "tool_use" && block.ToolUse != nil {
				toolUseIDToName[block.ToolUse.ID] = block.ToolUse.Function.Name
			}
		}
	}

	var contents []*genai.Content
	for _, msg := range req.Messages {
		role := "user"
		if msg.Role == "assistant" {
			role = "model"
		}

		var parts []genai.Part
		if len(msg.ContentBlocks) > 0 {
			parts = h.convertContentBlocks(msg.ContentBlocks, toolUseIDToName)
		} else {
			// Legacy fallback
			if msg.Content != "" {
				parts = append(parts, genai.Text(msg.Content))
			}
			if msg.Thinking != "" {
				parts = append(parts, genai.Text(msg.Thinking))
			}
			if msg.ToolResult != nil {
				name := toolUseIDToName[msg.ToolResult.ToolUseID]
				parts = append(parts, genai.FunctionResponse{
					Name: name,
					Response: map[string]any{
						"result": msg.ToolResult.Content,
					},
				})
			}
		}

		if len(parts) > 0 {
			contents = append(contents, &genai.Content{
				Role:  role,
				Parts: parts,
			})
		}
	}

	return contents
}

// convertContentBlocks converts content blocks to Gemini Parts.
// Replicates convertAnthropicContentToGemini from gemini-format.ts.
func (h *GeminiHandler) convertContentBlocks(blocks []ContentBlock, toolUseIDToName map[string]string) []genai.Part {
	var parts []genai.Part
	for _, block := range blocks {
		switch block.Type {
		case "text":
			parts = append(parts, genai.Text(block.Text))
		case "thinking":
			parts = append(parts, genai.Text(block.Thinking))
		case "image":
			if block.ImageSource != nil && block.ImageSource.Data != "" {
				data, err := base64.StdEncoding.DecodeString(block.ImageSource.Data)
				if err == nil {
					parts = append(parts, genai.ImageData(block.ImageSource.MimeType, data))
				}
			}
		case "tool_use":
			if block.ToolUse != nil {
				var args map[string]any
				if err := json.Unmarshal([]byte(block.ToolUse.Function.Arguments), &args); err != nil {
					args = map[string]any{}
				}
				parts = append(parts, genai.FunctionCall{
					Name: block.ToolUse.Function.Name,
					Args: args,
				})
			}
		case "tool_result":
			if block.ToolResult != nil {
				name := toolUseIDToName[block.ToolResult.ToolUseID]
				parts = append(parts, genai.FunctionResponse{
					Name: name,
					Response: map[string]any{
						"result": block.ToolResult.Content,
					},
				})
			}
		}
	}
	return parts
}

func (h *GeminiHandler) convertChunk(resp *genai.GenerateContentResponse) StreamChunk {
	if len(resp.Candidates) == 0 {
		return StreamChunk{Type: "delta"}
	}

	candidate := resp.Candidates[0]
	var textDelta string
	var contentBlocks []ContentBlock

	if candidate.Content != nil {
		for _, part := range candidate.Content.Parts {
			switch p := part.(type) {
			case genai.Text:
				textDelta += string(p)
				contentBlocks = append(contentBlocks, ContentBlock{Type: "text", Text: string(p)})
			case genai.FunctionCall:
				argsJSON, _ := json.Marshal(p.Args)
				contentBlocks = append(contentBlocks, ContentBlock{
					Type: "tool_use",
					ToolUse: &ToolUseBlock{
						ID:   p.Name, // Gemini doesn't have separate IDs
						Type: "tool_use",
						Function: struct {
							Name      string `json:"name"`
							Arguments string `json:"arguments"`
						}{Name: p.Name, Arguments: string(argsJSON)},
					},
				})
			}
		}
	}

	// Check finish reason
	if candidate.FinishReason != 0 && candidate.FinishReason.String() != "FINISH_REASON_UNSPECIFIED" {
		usage := &Usage{}
		if resp.UsageMetadata != nil {
			usage.InputTokens = int(resp.UsageMetadata.PromptTokenCount)
			usage.OutputTokens = int(resp.UsageMetadata.CandidatesTokenCount)
			usage.TotalTokens = int(resp.UsageMetadata.TotalTokenCount)
		}
		// Map Gemini finish reasons to standard ones
		stopReason := mapGeminiFinishReason(candidate.FinishReason)
		return StreamChunk{
			Type:          "stop",
			ContentBlocks: contentBlocks,
			FinishReason:  stopReason,
			Usage:         usage,
		}
	}

	return StreamChunk{
		Type:          "delta",
		TextDelta:     textDelta,
		Content:       textDelta,
		ContentBlocks: contentBlocks,
	}
}

func mapGeminiFinishReason(reason genai.FinishReason) string {
	switch reason.String() {
	case "STOP":
		return "end_turn"
	case "MAX_TOKENS":
		return "max_tokens"
	case "SAFETY", "RECITATION", "OTHER":
		return "stop_sequence"
	default:
		return string(reason)
	}
}

// jsonSchemaToGeminiSchema converts a JSON Schema object to a Gemini Schema struct.
func jsonSchemaToGeminiSchema(schemaJSON json.RawMessage) *genai.Schema {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(schemaJSON, &raw); err != nil {
		return &genai.Schema{Type: genai.TypeObject}
	}

	schema := &genai.Schema{
		Type: genai.TypeObject,
	}

	// Parse type
	if t, ok := raw["type"]; ok {
		var typeStr string
		json.Unmarshal(t, &typeStr)
		schema.Type = mapJSONTypeToGemini(typeStr)
	}

	// Parse description
	if d, ok := raw["description"]; ok {
		json.Unmarshal(d, &schema.Description)
	}

	// Parse properties
	if props, ok := raw["properties"]; ok {
		var propsMap map[string]json.RawMessage
		if json.Unmarshal(props, &propsMap) == nil {
			schema.Properties = make(map[string]*genai.Schema)
			for name, propJSON := range propsMap {
				schema.Properties[name] = jsonSchemaToGeminiSchema(propJSON)
			}
		}
	}

	// Parse required
	if req, ok := raw["required"]; ok {
		json.Unmarshal(req, &schema.Required)
	}

	return schema
}

func mapJSONTypeToGemini(t string) genai.Type {
	switch t {
	case "string":
		return genai.TypeString
	case "number":
		return genai.TypeNumber
	case "integer":
		return genai.TypeInteger
	case "boolean":
		return genai.TypeBoolean
	case "array":
		return genai.TypeArray
	case "object":
		return genai.TypeObject
	default:
		return genai.TypeObject
	}
}
