package providers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"net/http"
	"strings"
	"sync"
	"time"
)

// SharedHTTPClient is a reusable HTTP client with tuned transport settings.
// All provider calls should use this instead of creating throwaway http.Client instances.
var SharedHTTPClient *http.Client

func init() {
	SharedHTTPClient = &http.Client{
		Timeout: 30 * time.Second,
		Transport: &http.Transport{
			MaxIdleConns:        100,
			MaxIdleConnsPerHost: 10,
			IdleConnTimeout:     90 * time.Second,
		},
	}
}

const maxModelsCacheSize = 64

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
	Index         int             `json:"index"`
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

// ProviderAPIError is a structured error from a provider HTTP API call.
// Providers should return this for HTTP-level errors so the gateway can
// make informed retry decisions via IsRetriable.
type ProviderAPIError struct {
	StatusCode int
	Message    string
	Retriable  bool
}

func (e *ProviderAPIError) Error() string {
	return e.Message
}

// IsRetriable checks whether an error should be retried.
// Recognizes ProviderAPIError by type; falls back to substring matching
// for untyped fmt.Errorf errors from providers that haven't migrated yet.
func IsRetriable(err error) bool {
	if err == nil {
		return false
	}
	var pae *ProviderAPIError
	if errors.As(err, &pae) {
		return pae.Retriable
	}
	msg := err.Error()
	if strings.Contains(msg, "429") || strings.Contains(msg, "rate_limit") || strings.Contains(msg, "rate limit") {
		return true
	}
	if strings.Contains(msg, "500") || strings.Contains(msg, "502") || strings.Contains(msg, "503") || strings.Contains(msg, "504") {
		return true
	}
	return false
}

// ProviderMeta is a lightweight descriptor returned by list-providers.
type ProviderMeta struct {
	ID           string `json:"id"`
	Label        string `json:"label"`
	DefaultModel string `json:"default_model,omitempty"`
}

// modelsCacheEntry holds cached model list results with expiry.
type modelsCacheEntry struct {
	models []ModelEntry
	expiry time.Time
}

// Registry manages provider handlers
type Registry struct {
	handlers    map[string]Handler
	meta        map[string]ProviderMeta
	modelsCache map[string]modelsCacheEntry
	modelsMu    sync.RWMutex
}

// NewRegistry creates a new provider registry
func NewRegistry() *Registry {
	r := &Registry{
		handlers:    make(map[string]Handler),
		meta:        make(map[string]ProviderMeta),
		modelsCache: make(map[string]modelsCacheEntry),
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
		return ch.Capabilities().WithMaxTokensSetting()
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

// ListModels returns available models for a provider. Uses ModelLister if
// implemented, with an in-memory TTL cache (1 hour). Returns nil if the
// provider doesn't support model discovery.
func (r *Registry) ListModels(ctx context.Context, providerID string, cfg ProviderConfig) ([]ModelEntry, error) {
	handler, ok := r.handlers[providerID]
	if !ok {
		return nil, errors.New("unsupported provider: " + providerID)
	}
	ml, ok := handler.(ModelLister)
	if !ok {
		return nil, nil
	}

	// Check cache
	cacheKey := providerID + ":" + cfg.BaseURL
	r.modelsMu.RLock()
	if entry, hit := r.modelsCache[cacheKey]; hit && time.Now().Before(entry.expiry) {
		r.modelsMu.RUnlock()
		return entry.models, nil
	}
	r.modelsMu.RUnlock()

	// Fetch from provider
	models, err := ml.ListModels(ctx, cfg)
	if err != nil {
		return nil, err
	}

	// Cache for 1 hour, evict oldest if at capacity
	r.modelsMu.Lock()
	if len(r.modelsCache) >= maxModelsCacheSize {
		var oldestKey string
		var oldestTime time.Time
		for k, v := range r.modelsCache {
			if oldestKey == "" || v.expiry.Before(oldestTime) {
				oldestKey = k
				oldestTime = v.expiry
			}
		}
		if oldestKey != "" {
			delete(r.modelsCache, oldestKey)
		}
	}
	r.modelsCache[cacheKey] = modelsCacheEntry{
		models: models,
		expiry: time.Now().Add(1 * time.Hour),
	}
	r.modelsMu.Unlock()

	return models, nil
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

// Register registers a handler for a provider with metadata.
func (r *Registry) Register(providerID string, handler Handler, meta ProviderMeta) {
	r.handlers[providerID] = handler
	// Fill DefaultModel from Capabilities if not set in meta
	if meta.DefaultModel == "" {
		if ch, ok := handler.(CapableHandler); ok {
			if caps := ch.Capabilities(); caps != nil {
				meta.DefaultModel = caps.DefaultModel
			}
		}
	}
	r.meta[providerID] = meta
}

// SupportedProviders returns the metadata for all registered providers.
func (r *Registry) SupportedProviders() []ProviderMeta {
	providers := make([]ProviderMeta, 0, len(r.meta))
	for _, m := range r.meta {
		providers = append(providers, m)
	}
	return providers
}

// registerProviders registers all built-in providers
func (r *Registry) registerProviders() {
	r.Register("anthropic", NewAnthropicHandler(), ProviderMeta{ID: "anthropic", Label: "Anthropic"})
	r.Register("openai", NewOpenAIHandler(), ProviderMeta{ID: "openai", Label: "OpenAI"})
	r.Register("openrouter", NewOpenRouterHandler(), ProviderMeta{ID: "openrouter", Label: "OpenRouter"})
	r.Register("gemini", NewGeminiHandler(), ProviderMeta{ID: "gemini", Label: "Google Gemini"})
	r.Register("minimax", NewMiniMaxHandler(), ProviderMeta{ID: "minimax", Label: "MiniMax"})
	r.Register("zai", NewZAIHandler(), ProviderMeta{ID: "zai", Label: "Z AI"})
	r.Register("deepseek", NewDeepSeekHandler(), ProviderMeta{ID: "deepseek", Label: "DeepSeek"})
	r.Register("mistral", NewMistralHandler(), ProviderMeta{ID: "mistral", Label: "Mistral"})
	r.Register("groq", NewGroqHandler(), ProviderMeta{ID: "groq", Label: "Groq"})
	r.Register("xai", NewXAIHandler(), ProviderMeta{ID: "xai", Label: "xAI"})
	r.Register("qwen", NewQwenHandler(), ProviderMeta{ID: "qwen", Label: "Qwen"})
	r.Register("fireworks", NewFireworksHandler(), ProviderMeta{ID: "fireworks", Label: "Fireworks"})
	r.Register("together", NewTogetherHandler(), ProviderMeta{ID: "together", Label: "Together"})
	r.Register("sambanova", NewSambaNovaHandler(), ProviderMeta{ID: "sambanova", Label: "SambaNova"})
	r.Register("cerebras", NewCerebrasHandler(), ProviderMeta{ID: "cerebras", Label: "Cerebras"})
	r.Register("lmstudio", NewLmStudioHandler(), ProviderMeta{ID: "lmstudio", Label: "LM Studio"})
	r.Register("moonshot", NewMoonshotHandler(), ProviderMeta{ID: "moonshot", Label: "Moonshot"})
	r.Register("nvidia-nim", NewNvidiaNimHandler(), ProviderMeta{ID: "nvidia-nim", Label: "NVIDIA NIM"})
	r.Register("nebius", NewNebiusHandler(), ProviderMeta{ID: "nebius", Label: "Nebius"})
	r.Register("huggingface", NewHuggingFaceHandler(), ProviderMeta{ID: "huggingface", Label: "Hugging Face"})
	r.Register("opencode_go", NewOpenCodeGoHandler(), ProviderMeta{ID: "opencode_go", Label: "OpenCode Go"})
	r.Register("opencode_zen", NewOpenCodeZenHandler(), ProviderMeta{ID: "opencode_zen", Label: "OpenCode Zen"})
	r.Register("kilocode", NewKiloCodeHandler(), ProviderMeta{ID: "kilocode", Label: "KiloCode"})
	r.Register("byteplus", NewBytePlusHandler(), ProviderMeta{ID: "byteplus", Label: "BytePlus"})
	r.Register("byteplus_coding_plan", NewBytePlusCodingPlanHandler(), ProviderMeta{ID: "byteplus_coding_plan", Label: "BytePlus Coding Plan"})
	r.Register("openai_codex", NewOpenAICodexHandler(), ProviderMeta{ID: "openai_codex", Label: "OpenAI Codex"})
	r.Register("xiaomi_mimo", NewXiaomiMimoHandler(), ProviderMeta{ID: "xiaomi_mimo", Label: "Xiaomi MiMo"})
	r.Register("synthetic", NewSyntheticHandler(), ProviderMeta{ID: "synthetic", Label: "Synthetic"})
	r.Register("wafer", NewWaferHandler(), ProviderMeta{ID: "wafer", Label: "Wafer"})
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
