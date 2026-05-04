package providers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
)

// Request represents a standardized API request
type Request struct {
	Provider         ProviderConfig
	Messages         []Message
	System           string
	Tools            []json.RawMessage
	MaxTokens        int
	Temperature      float64
	TopP             float64
	Stop             []string
	ModelOverride    string
	Thinking         *ThinkingConfig
	Logprobs         bool    `json:"logprobs,omitempty"`
	TopLogprobs      int     `json:"top_logprobs,omitempty"`
	PresencePenalty  float64 `json:"presence_penalty,omitempty"`
	FrequencyPenalty float64 `json:"frequency_penalty,omitempty"`
	Settings         map[string]interface{} `json:"settings,omitempty"`
}

// SettingString returns a setting value as string, or "" if not set.
func (r *Request) SettingString(key string) string {
	if r.Settings == nil {
		return ""
	}
	if v, ok := r.Settings[key]; ok {
		return fmt.Sprint(v)
	}
	return ""
}

// SettingFloat returns a setting value as float64, or 0 if not set.
func (r *Request) SettingFloat(key string) float64 {
	if r.Settings == nil {
		return 0
	}
	if v, ok := r.Settings[key]; ok {
		if f, ok := v.(float64); ok {
			return f
		}
	}
	return 0
}

// SettingBool returns a setting value as bool, or false if not set.
func (r *Request) SettingBool(key string) bool {
	if r.Settings == nil {
		return false
	}
	if v, ok := r.Settings[key]; ok {
		if b, ok := v.(bool); ok {
			return b
		}
	}
	return false
}

// SettingInt returns a setting value as int64, or 0 if not set.
func (r *Request) SettingInt(key string) int64 {
	if r.Settings == nil {
		return 0
	}
	if v, ok := r.Settings[key]; ok {
		if f, ok := v.(float64); ok {
			return int64(f)
		}
		if i, ok := v.(int64); ok {
			return i
		}
	}
	return 0
}

// ProviderConfig contains the provider-specific configuration
type ProviderConfig struct {
	ID        string `json:"id"`
	APIKey    string `json:"api_key,omitempty"`
	BaseURL   string `json:"base_url,omitempty"`
	Model     string `json:"model,omitempty"`
	Region    string `json:"region,omitempty"`
	ProjectID string `json:"project_id,omitempty"`
	Extra     map[string]interface{} `json:"extra,omitempty"`
}

// Message represents a conversation message
type Message struct {
	Role          string         `json:"role"`
	Content       string         `json:"content,omitempty"`
	ContentBlocks []ContentBlock `json:"content_blocks,omitempty"`
	ToolCalls     []ToolCall     `json:"tool_calls,omitempty"`
	ToolUseID     string         `json:"tool_use_id,omitempty"`
	Thinking      string         `json:"thinking,omitempty"`
	Name          string         `json:"name,omitempty"`
	ToolResult    *ToolResult    `json:"tool_result,omitempty"`
}

// ToolCall represents a tool call from the model
type ToolCall struct {
	ID       string       `json:"id"`
	Type     string       `json:"type"`
	Function FunctionCall `json:"function"`
}

// FunctionCall represents the function call details
type FunctionCall struct {
	Name      string `json:"name"`
	Arguments string `json:"arguments"`
}

// ToolResult represents the result of a tool call
type ToolResult struct {
	ToolUseID string `json:"tool_use_id"`
	Content   string `json:"content"`
	IsError   bool   `json:"is_error,omitempty"`
}

// ToolResultBlock represents a tool result content block (with type info)
type ToolResultBlock struct {
	Type      string `json:"type"`
	ToolUseID string `json:"tool_use_id,omitempty"`
	Content   string `json:"content,omitempty"`
	IsError   bool   `json:"is_error,omitempty"`
}

// ImageSourceBlock represents an image source content block
type ImageSourceBlock struct {
	Type     string `json:"type"`
	MimeType string `json:"mime_type,omitempty"`
	Data     string `json:"data,omitempty"`
	URL      string `json:"url,omitempty"`
}

// ThinkingConfig configures extended thinking
type ThinkingConfig struct {
	Type            string `json:"type"`
	BudgetTokens    int    `json:"budget_tokens,omitempty"`
	ReasoningEffort string `json:"reasoning_effort,omitempty"` // "high" or "max" (DeepSeek, OpenAI o-series)
}

// StreamChunk represents a streaming response chunk with typed delta fields
type StreamChunk struct {
	Type          string          `json:"type"`
	Index         int             `json:"index,omitempty"`
	TextDelta     string          `json:"text_delta,omitempty"`
	JSONDelta     string          `json:"json_delta,omitempty"`
	ToolCallID    string          `json:"tool_call_id,omitempty"`
	ToolCallName  string          `json:"tool_call_name,omitempty"`
	Thinking      string          `json:"thinking,omitempty"`
	Usage         *Usage          `json:"usage,omitempty"`
	FinishReason  string          `json:"finish_reason,omitempty"`
	ContentBlocks []ContentBlock  `json:"content_blocks,omitempty"`
	Content       string          `json:"content,omitempty"`
}

// Handler is the interface for all API providers
type Handler interface {
	Send(ctx context.Context, req *Request) (*SendResult, error)
	Stream(ctx context.Context, req *Request, callback func(StreamChunk) error) error
}

// SendResult represents the result of a non-streaming request
type SendResult struct {
	Content    []ContentBlock `json:"content,omitempty"`
	StopReason string         `json:"stop_reason,omitempty"`
	Usage      *Usage         `json:"usage,omitempty"`
	Model      string         `json:"model,omitempty"`
	Raw        json.RawMessage `json:"raw,omitempty"`
	Error      *ProviderError `json:"error,omitempty"`
}

// ContentBlock represents a content block in the response
type ContentBlock struct {
	Type        string           `json:"type"`
	Text        string           `json:"text,omitempty"`
	ToolUse     *ToolUseBlock    `json:"tool_use,omitempty"`
	Thinking    string           `json:"thinking,omitempty"`
	Signature   string           `json:"signature,omitempty"`
	ToolResult  *ToolResultBlock `json:"tool_result,omitempty"`
	ImageSource *ImageSourceBlock `json:"image_source,omitempty"`
}

// ToolUseBlock represents a tool use block
type ToolUseBlock struct {
	ID       string `json:"id"`
	Type     string `json:"type"`
	Function struct {
		Name      string `json:"name"`
		Arguments string `json:"arguments"`
	} `json:"function"`
}

// Usage represents API usage statistics
type Usage struct {
	InputTokens  int `json:"input_tokens"`
	OutputTokens int `json:"output_tokens"`
	TotalTokens  int `json:"total_tokens"`
	CacheCreationInputTokens int `json:"cache_creation_input_tokens,omitempty"`
	CacheReadInputTokens     int `json:"cache_read_input_tokens,omitempty"`
	ReasoningTokens          int `json:"reasoning_tokens,omitempty"`
}

// ProviderError represents an error from the provider
type ProviderError struct {
	Type    string `json:"type"`
	Message string `json:"message"`
	Code    int    `json:"code,omitempty"`
}

// Registry manages provider handlers
type Registry struct {
	handlers map[string]Handler
}

// NewRegistry creates a new provider registry
func NewRegistry() *Registry {
	r := &Registry{
		handlers: make(map[string]Handler),
	}
	r.registerProviders()
	return r
}

// GetHandler returns the handler for a provider
func (r *Registry) GetHandler(providerID string) (Handler, error) {
	handler, ok := r.handlers[providerID]
	if !ok {
		return nil, errors.New("unsupported provider: " + providerID)
	}
	return handler, nil
}

// GetCapabilities returns capability info for a provider, or nil if
// the handler doesn't implement CapableHandler.
func (r *Registry) GetCapabilities(providerID string) *ProviderInfo {
	handler, ok := r.handlers[providerID]
	if !ok {
		return nil
	}
	if ch, ok := handler.(CapableHandler); ok {
		return ch.Capabilities()
	}
	return nil
}

// ValidateSettings validates the given settings for a provider, returning
// per-parameter status, corrected values, and error messages.
// If the handler implements SettingsValidator, delegates to it.
// Otherwise, performs generic validation from the ProviderInfo schema.
func (r *Registry) ValidateSettings(providerID string, settings map[string]interface{}, thinking *ThinkingConfig) *ValidateSettingsResult {
	handler, ok := r.handlers[providerID]
	if !ok {
		return nil
	}
	if sv, ok := handler.(SettingsValidator); ok {
		return sv.ValidateSettings(settings, thinking)
	}
	// Generic validation from ProviderInfo schema
	info := r.GetCapabilities(providerID)
	if info == nil || len(info.Settings) == 0 {
		return nil
	}
	result := &ValidateSettingsResult{Settings: make(map[string]SettingValidation)}
	for _, s := range info.Settings {
		v := SettingValidation{Status: StatusActive}
		if s.Type == SettingSlider {
			num := toFloat(settings[s.Key])
			if num == 0 {
				num = floatDefault(s.Default, 0)
			}
			clamped := num
			if s.Min != nil {
				clamped = math.Max(clamped, *s.Min)
			}
			if s.Max != nil {
				clamped = math.Min(clamped, *s.Max)
			}
			if clamped != num {
				v.Value = clamped
			}
		}
		result.Settings[s.Key] = v
	}
	return result
}

// toFloat converts an interface{} to float64.
func toFloat(v interface{}) float64 {
	if v == nil {
		return 0
	}
	if f, ok := v.(float64); ok {
		return f
	}
	return 0
}

// floatDefault returns the float64 value of def, or fallback.
func floatDefault(def interface{}, fallback float64) float64 {
	if def == nil {
		return fallback
	}
	if f, ok := def.(float64); ok {
		return f
	}
	return fallback
}

// Register registers a handler for a provider
func (r *Registry) Register(providerID string, handler Handler) {
	r.handlers[providerID] = handler
}

// SupportedProviders returns the list of supported providers
func (r *Registry) SupportedProviders() []string {
	providers := make([]string, 0, len(r.handlers))
	for id := range r.handlers {
		providers = append(providers, id)
	}
	return providers
}

// registerProviders registers all built-in providers
func (r *Registry) registerProviders() {
	r.Register("anthropic", NewAnthropicHandler())
	r.Register("openai", NewOpenAIHandler())
	r.Register("openrouter", NewOpenRouterHandler())
	r.Register("gemini", NewGeminiHandler())
	r.Register("minimax", NewMiniMaxHandler())
	r.Register("zai", NewZAIHandler())
	r.Register("deepseek", NewDeepSeekHandler())
	r.Register("mistral", NewMistralHandler())
	r.Register("groq", NewGroqHandler())
	r.Register("xai", NewXAIHandler())
	r.Register("qwen", NewQwenHandler())
	r.Register("fireworks", NewFireworksHandler())
	r.Register("together", NewTogetherHandler())
	r.Register("sambanova", NewSambaNovaHandler())
	r.Register("cerebras", NewCerebrasHandler())
	r.Register("lmstudio", NewLmStudioHandler())
	r.Register("moonshot", NewMoonshotHandler())
	r.Register("nvidia-nim", NewNvidiaNimHandler())
	r.Register("nebius", NewNebiusHandler())
	r.Register("huggingface", NewHuggingFaceHandler())
}

// ValidateRequest validates a request before processing
// Updated to consider ContentBlocks (and Thinking) when checking for content presence
func ValidateRequest(req *Request) error {
	if req.Provider.ID == "" {
		return errors.New("provider ID is required")
	}
	if len(req.Messages) == 0 {
		return errors.New("at least one message is required")
	}
	// Validate messages
	for i, msg := range req.Messages {
		if msg.Role == "" {
			return fmt.Errorf("message at index %d has no role", i)
		}
		// Check for content presence: legacy Content field, ContentBlocks, or Thinking
		hasContent := msg.Content != ""
		hasContentBlocks := len(msg.ContentBlocks) > 0
		hasThinking := msg.Thinking != ""
		hasToolCalls := len(msg.ToolCalls) > 0
		hasToolResult := msg.ToolResult != nil

		if !hasContent && !hasContentBlocks && !hasThinking && !hasToolCalls && !hasToolResult {
			return fmt.Errorf("message at index %d has no content", i)
		}
	}
	return nil
}

// ConvertMessages converts internal messages to provider format
func ConvertMessages(msgs []Message) interface{} {
	return msgs
}
