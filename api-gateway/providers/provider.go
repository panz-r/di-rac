package providers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"net"
	"net/http"
	"sort"
	"sync"
	"time"

	"golang.org/x/sync/singleflight"
)

// SharedHTTPClient is a reusable HTTP client with tuned transport settings.
// All provider calls should use this instead of creating throwaway http.Client instances.
var SharedHTTPClient *http.Client

func init() {
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.MaxIdleConns = 20
	transport.MaxIdleConnsPerHost = 5
	transport.IdleConnTimeout = 90 * time.Second
	transport.ResponseHeaderTimeout = 60 * time.Second
	// Wrap DialContext to block connections to private/internal IPs at dial time.
	// This prevents DNS rebinding attacks where DNS resolves to a public IP during
	// pre-validation but switches to a private IP before the actual connection.
	origDial := transport.DialContext
	transport.DialContext = func(ctx context.Context, network, addr string) (net.Conn, error) {
		host, _, err := net.SplitHostPort(addr)
		if err != nil {
			return nil, fmt.Errorf("invalid address %q: %w", addr, err)
		}
		// Allow localhost for local providers (Ollama, LM Studio, etc.)
		if host == "localhost" || host == "127.0.0.1" || host == "::1" {
			if origDial != nil {
				return origDial(ctx, network, addr)
			}
			var d net.Dialer
			return d.DialContext(ctx, network, addr)
		}
		// Resolve and check before dialing. This runs at connection time,
		// closing the TOCTOU gap between pre-validation and actual dial.
		ips, err := net.DefaultResolver.LookupIPAddr(ctx, host)
		if err != nil {
			return nil, fmt.Errorf("DNS lookup for %q: %w", host, err)
		}
		for _, ip := range ips {
			if ip.IP.IsPrivate() || ip.IP.IsLinkLocalUnicast() || ip.IP.IsLoopback() {
				return nil, fmt.Errorf("blocked connection to private/internal IP %s (host %q)", ip.IP, host)
			}
		}
		// All IPs are safe — dial using the original dialer.
		if origDial != nil {
			return origDial(ctx, network, addr)
		}
		var d net.Dialer
		return d.DialContext(ctx, network, addr)
	}
	SharedHTTPClient = &http.Client{
		Transport: transport,
		// No client-level Timeout: per-request context deadlines control
		// stream lifetime. A fixed Timeout would abort long-running streams
		// (deep reasoning, large context) before the context expires.
	}
}

const maxModelsCacheSize = 32

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

// SettingIsNull returns true if the key exists with a nil value,
// meaning the user explicitly excluded this parameter.
func (r *Request) SettingIsNull(key string) bool {
	if r.Settings == nil {
		return false
	}
	v, ok := r.Settings[key]
	return ok && v == nil
}

// SettingString returns a setting value as string, or "" if not set or null.
func (r *Request) SettingString(key string) string {
	if r.Settings == nil {
		return ""
	}
	if v, ok := r.Settings[key]; ok {
		if v == nil {
			return ""
		}
		return fmt.Sprint(v)
	}
	return ""
}

// SettingFloat returns a setting value as float64, or 0 if not set.
// Handles float64 (JSON numbers), int, and int64 types.
func (r *Request) SettingFloat(key string) float64 {
	if r.Settings == nil {
		return 0
	}
	if v, ok := r.Settings[key]; ok {
		switch n := v.(type) {
		case float64:
			return n
		case int:
			return float64(n)
		case int64:
			return float64(n)
		}
	}
	return 0
}

// ApplySettingFloat sets result[key] from the settings map, handling three cases:
//   - key absent from settings: no-op (preserve provider default)
//   - key present with nil: delete result[key] (user explicitly excluded)
//   - key present with value: set result[key] = value
func (r *Request) ApplySettingFloat(result map[string]interface{}, key string) {
	if r.Settings == nil {
		return
	}
	v, ok := r.Settings[key]
	if !ok {
		return // absent — don't touch result, let provider default stand
	}
	if v == nil {
		delete(result, key)
		return
	}
	switch n := v.(type) {
	case float64:
		result[key] = n
	case int:
		result[key] = float64(n)
	case int64:
		result[key] = float64(n)
	}
}

// SettingBool returns a setting value as bool, or false if not set.
func (r *Request) SettingBool(key string) bool {
	b, _ := r.SettingBoolOK(key)
	return b
}

// SettingBoolOK returns a setting value as bool and whether it was explicitly set.
func (r *Request) SettingBoolOK(key string) (bool, bool) {
	if r.Settings == nil {
		return false, false
	}
	v, ok := r.Settings[key]
	if !ok || v == nil {
		return false, ok
	}
	if b, ok := v.(bool); ok {
		return b, true
	}
	return false, false
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
	// RetryAfter, if non-zero, is the provider-suggested backoff duration
	// parsed from a Retry-After header on 429 responses.
	RetryAfter time.Duration
	// ContextExceeded is true when the provider rejected the request because
	// the total prompt exceeded its context window. Providers set this when
	// they detect their own provider-specific context-limit response.
	ContextExceeded bool
}

func (e *ProviderAPIError) Error() string {
	return e.Message
}

// IsRetriable checks whether an error should be retried.
// Relies solely on ProviderAPIError.Retriable; untyped errors are not retried.
func IsRetriable(err error) bool {
	if err == nil {
		return false
	}
	var pae *ProviderAPIError
	if errors.As(err, &pae) {
		return pae.Retriable
	}
	return false
}

// wrapTransientError wraps a network/transport error as a retriable ProviderAPIError.
// If the error is already a ProviderAPIError it is returned unchanged.
func wrapTransientError(err error) error {
	if err == nil {
		return nil
	}
	if _, ok := err.(*ProviderAPIError); ok {
		return err
	}
	return &ProviderAPIError{
		StatusCode: 0,
		Message:    err.Error(),
		Retriable:  true,
	}
}

// IsContextExceeded returns true if the error indicates the provider's context
// window was exceeded. Each provider detects this from its own error format.
func IsContextExceeded(err error) bool {
	if err == nil {
		return false
	}
	var pae *ProviderAPIError
	if errors.As(err, &pae) {
		return pae.ContextExceeded
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

// Registry manages provider handlers.
// Thread safety: handlers and meta are populated once during NewRegistry and
// only read afterward. modelsCache is guarded by modelsMu.
type Registry struct {
	handlers    map[string]Handler
	meta        map[string]ProviderMeta
	caps        map[string]*ProviderInfo
	modelsCache map[string]modelsCacheEntry
	modelsMu    sync.RWMutex
	modelsSF    singleflight.Group
}

// NewRegistry creates a new provider registry
func NewRegistry() *Registry {
	r := &Registry{
		handlers:    make(map[string]Handler),
		meta:        make(map[string]ProviderMeta),
		caps:        make(map[string]*ProviderInfo),
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

// GetCapabilities returns pre-computed capability info for a provider,
// or nil if the handler doesn't implement CapableHandler.
func (r *Registry) GetCapabilities(providerID string) *ProviderInfo {
	return r.caps[providerID]
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

	// Check cache (include API key so different tiers see different model lists)
	cacheKey := providerID + ":" + cfg.BaseURL + ":" + cfg.APIKey
	r.modelsMu.RLock()
	if entry, hit := r.modelsCache[cacheKey]; hit && time.Now().Before(entry.expiry) {
		r.modelsMu.RUnlock()
		// Return a copy so callers can't mutate the cached slice.
		cp := make([]ModelEntry, len(entry.models))
		copy(cp, entry.models)
		return cp, nil
	}
	r.modelsMu.RUnlock()

	// Deduplicate concurrent requests for the same provider+baseURL.
	// Use context.Background() inside the fn so one caller's cancellation
	// doesn't poison all other callers waiting on the same singleflight key.
	// However, if the caller's context expires first, forget the key so
	// subsequent callers can retry instead of inheriting the stale result.
	ch := r.modelsSF.DoChan(cacheKey, func() (interface{}, error) {
		// Bound the listing call to prevent hung providers from blocking
		// all callers sharing this singleflight key forever.
		listCtx, listCancel := context.WithTimeout(context.Background(), 60*time.Second)
		defer listCancel()
		return ml.ListModels(listCtx, cfg)
	})
	var v interface{}
	var err error
	select {
	case <-ctx.Done():
		r.modelsSF.Forget(cacheKey)
		return nil, ctx.Err()
	case res := <-ch:
		v, err = res.Val, res.Err
	}
	if err != nil {
		return nil, err
	}
	models, ok := v.([]ModelEntry)
		if !ok || models == nil {
			return nil, fmt.Errorf("unexpected type from model lister")
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

	// Return a copy so callers cannot mutate the cached slice.
	cp := make([]ModelEntry, len(models))
	copy(cp, models)
	return cp, nil
}

// toFloat converts an interface{} to float64.
// Handles float64 (JSON numbers), int, and int64 types.
func toFloat(v interface{}) float64 {
	if v == nil {
		return 0
	}
	switch n := v.(type) {
	case float64:
		return n
	case int:
		return float64(n)
	case int64:
		return float64(n)
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
	// Pre-compute capabilities once (including max_tokens setting) to avoid
	// per-request allocation from WithMaxTokensSetting().
	if ch, ok := handler.(CapableHandler); ok {
		if caps := ch.Capabilities(); caps != nil {
			r.caps[providerID] = caps.WithMaxTokensSetting()
			if meta.DefaultModel == "" {
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
	sort.Slice(providers, func(i, j int) bool {
		return providers[i].ID < providers[j].ID
	})
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
	r.Register("venice", NewVeniceHandler(), ProviderMeta{ID: "venice", Label: "Venice AI"})
	r.Register("inference_net", NewInferenceNetHandler(), ProviderMeta{ID: "inference_net", Label: "Inference.net"})
	r.Register("ovhcloud", NewOVHcloudHandler(), ProviderMeta{ID: "ovhcloud", Label: "OVHcloud AI Endpoints"})
	r.Register("ollama", NewOllamaHandler(), ProviderMeta{ID: "ollama", Label: "Ollama"})
	r.Register("replicate", NewReplicateHandler(), ProviderMeta{ID: "replicate", Label: "Replicate"})
}

// ValidateRequest validates a request before processing.
// Checks basic message structure and conversation-level tool invariants.
func ValidateRequest(req *Request) error {
	if req.Provider.ID == "" {
		return errors.New("provider ID is required")
	}
	if len(req.Messages) == 0 {
		return errors.New("at least one message is required")
	}
	if len(req.Messages) > 1000 {
		return fmt.Errorf("too many messages: %d (max 1000)", len(req.Messages))
	}
	if len(req.Tools) > 128 {
		return fmt.Errorf("too many tools: %d (max 128)", len(req.Tools))
	}
	if req.MaxTokens > 1_000_000 {
		return fmt.Errorf("max_tokens too large: %d (max 1,000,000)", req.MaxTokens)
	}
	if len(req.Settings) > 100 {
		return fmt.Errorf("too many settings keys: %d (max 100)", len(req.Settings))
	}

	pendingToolCalls := make(map[string]string) // id -> name

	for i, msg := range req.Messages {
		if msg.Role == "" {
			return fmt.Errorf("message at index %d has no role", i)
		}
		if len(msg.ToolCalls) > 100 {
			return fmt.Errorf("message at index %d has too many tool calls: %d (max 100)", i, len(msg.ToolCalls))
		}
		if len(msg.ContentBlocks) > 100 {
			return fmt.Errorf("message at index %d has too many content blocks: %d (max 100)", i, len(msg.ContentBlocks))
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

		// Validate tool calls from legacy ToolCalls field
		for _, tc := range msg.ToolCalls {
			if tc.ID == "" {
				return fmt.Errorf("message at index %d has tool call with no id", i)
			}
			if _, exists := pendingToolCalls[tc.ID]; exists {
				return fmt.Errorf("duplicate tool_use id %q", tc.ID)
			}
			pendingToolCalls[tc.ID] = tc.Function.Name
		}

		// Validate tool results from legacy ToolResult field
		if msg.ToolResult != nil {
			if msg.ToolResult.ToolUseID == "" {
				return fmt.Errorf("message at index %d has tool_result with no tool_use_id", i)
			}
			if _, exists := pendingToolCalls[msg.ToolResult.ToolUseID]; !exists {
				return fmt.Errorf("tool_result at message %d references unknown tool_use_id %q", i, msg.ToolResult.ToolUseID)
			}
			delete(pendingToolCalls, msg.ToolResult.ToolUseID)
		}

		// Validate content blocks (tool_use / tool_result pairing)
		for j, block := range msg.ContentBlocks {
			switch block.Type {
			case "image":
				if block.ImageSource != nil && block.ImageSource.Data != "" {
					// Base64 is ~4/3 the binary size; 10MB binary ≈ 13.3MB base64.
					const maxBase64ImageSize = 14 << 20
					if len(block.ImageSource.Data) > maxBase64ImageSize {
						return fmt.Errorf("message %d block %d: image data exceeds 10MB limit", i, j)
					}
				}
			case "tool_use":
				if block.ToolUse == nil || block.ToolUse.ID == "" {
					return fmt.Errorf("message %d block %d: tool_use missing id", i, j)
				}
				if _, exists := pendingToolCalls[block.ToolUse.ID]; exists {
					return fmt.Errorf("duplicate tool_use id %q", block.ToolUse.ID)
				}
				pendingToolCalls[block.ToolUse.ID] = block.ToolUse.Function.Name
			case "tool_result":
				if block.ToolResult == nil || block.ToolResult.ToolUseID == "" {
					return fmt.Errorf("message %d block %d: tool_result missing tool_use_id", i, j)
				}
				if _, exists := pendingToolCalls[block.ToolResult.ToolUseID]; !exists {
					return fmt.Errorf("tool_result at message %d block %d references unknown tool_use_id %q", i, j, block.ToolResult.ToolUseID)
				}
				delete(pendingToolCalls, block.ToolResult.ToolUseID)
			}
		}
	}
	return nil
}
