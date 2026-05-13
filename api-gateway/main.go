package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/netip"
	"net/url"
	"os"
	"os/signal"
	"path/filepath"
	"runtime/debug"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/dirac-dev/api-gateway/providers"
)

const Version = "0.1.0"

// Rate limit configuration.
// Priority: environment variable > ldflags > default.
// Env vars: DIRAC_API_GATEWAY_RATE_PER_SEC, DIRAC_API_GATEWAY_MAX_CONCURRENT
var maxRequestsPerSecStr string = "5"
var maxInflightReqsStr string = "3"

func resolveRateLimits() (ratePerSec, maxConcurrent int) {
	ratePerSec = parseLimit(maxRequestsPerSecStr, 5)
	maxConcurrent = parseLimit(maxInflightReqsStr, 3)
	if v := os.Getenv("DIRAC_API_GATEWAY_RATE_PER_SEC"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 {
			ratePerSec = n
		}
	}
	if v := os.Getenv("DIRAC_API_GATEWAY_MAX_CONCURRENT"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 {
			maxConcurrent = n
		}
	}
	return
}

// parseLimit parses a build-time string as an int, returning defVal on failure.
func parseLimit(s string, defVal int) int {
	if n, err := strconv.Atoi(s); err == nil && n > 0 {
		return n
	}
	return defVal
}

var SocketPath = os.Getenv("DIRAC_API_GATEWAY_SOCKET")

func init() {
	if SocketPath == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			home = "/tmp"
		}
		SocketPath = home + "/.dirac/api-gateway.sock"
	}
}

// Request represents an API request with ContentBlocks support
type Request struct {
	ID            int64                     `json:"id"`
	Type          string                    `json:"type,omitempty"`
	Stream        bool                      `json:"stream,omitempty"`
	Timeout       int                       `json:"timeout,omitempty"`
	Provider      providers.ProviderConfig  `json:"provider"`
	Messages      []providers.Message       `json:"messages"`
	System        string                    `json:"system,omitempty"`
	Tools         []json.RawMessage         `json:"tools,omitempty"`
	MaxTokens     int                       `json:"max_tokens,omitempty"`
	Temperature   float64                   `json:"temperature,omitempty"`
	TopP          float64                   `json:"top_p,omitempty"`
	Stop          []string                  `json:"stop,omitempty"`
	Thinking         *providers.ThinkingConfig `json:"thinking,omitempty"`
	ModelOverride    string                    `json:"model_override,omitempty"`
	Logprobs         bool                      `json:"logprobs,omitempty"`
	TopLogprobs      int                       `json:"top_logprobs,omitempty"`
	PresencePenalty  float64                   `json:"presence_penalty,omitempty"`
	FrequencyPenalty float64                   `json:"frequency_penalty,omitempty"`
	Settings         map[string]interface{}    `json:"settings,omitempty"`
}

// SetProviderRequest handles provider configuration
type SetProviderRequest struct {
	Type     string                   `json:"type"`
	Provider string                   `json:"provider"`
	Config   providers.ProviderConfig `json:"config"`
}

// Response represents an API response
type Response struct {
	ID     int64           `json:"id"`
	Status int             `json:"status"`
	Body   json.RawMessage `json:"body,omitempty"`
	Error  *ErrorDetail    `json:"error,omitempty"`
}

// ErrorDetail represents error information
type ErrorDetail struct {
	Code      string `json:"code,omitempty"`
	Message   string `json:"message"`
	Retriable bool   `json:"retriable,omitempty"`
}

// responseWriter wraps a connection and JSON encoder with write deadlines.
type responseWriter struct {
	conn    net.Conn
	encoder *json.Encoder
	mu      sync.Mutex
}

func (w *responseWriter) write(v interface{}) error {
	w.mu.Lock()
	defer w.mu.Unlock()
	if err := w.conn.SetWriteDeadline(time.Now().Add(30 * time.Second)); err != nil {
		return fmt.Errorf("set write deadline: %w", err)
	}
	return w.encoder.Encode(v)
}

// Server manages the API gateway
type Server struct {
	providerRegistry *providers.Registry
	listener         net.Listener
	wg               sync.WaitGroup
	ctx              context.Context
	cancel           context.CancelFunc
	providerConfigs  map[string]providers.ProviderConfig
	configMu         sync.RWMutex
	rateLimiter      *RateLimiter
	conns            map[net.Conn]struct{}
	connMu           sync.Mutex
}

// RateLimiter controls outbound request rate and concurrency.
// Token bucket for rate limiting, buffered channel for max-inflight.
type RateLimiter struct {
	tokens chan struct{} // token bucket for req/s rate limiting
	sem    chan struct{} // semaphore for max concurrent requests
	done   chan struct{}
}

func NewRateLimiter(ratePerSec, maxConcurrent int) *RateLimiter {
	if ratePerSec <= 0 {
		ratePerSec = 5
	}
	if maxConcurrent <= 0 {
		maxConcurrent = 3
	}
	rl := &RateLimiter{
		tokens: make(chan struct{}, ratePerSec),
		sem:    make(chan struct{}, maxConcurrent),
		done:   make(chan struct{}),
	}
	// Fill initial burst allowance
	for i := 0; i < ratePerSec; i++ {
		rl.tokens <- struct{}{}
	}
	// Drip tokens at steady rate
	interval := time.Second / time.Duration(ratePerSec)
	go func() {
		ticker := time.NewTicker(interval)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				select {
				case rl.tokens <- struct{}{}:
				default: // bucket full
				}
			case <-rl.done:
				return
			}
		}
	}()
	return rl
}

// Wait blocks until a rate-limit token and an inflight slot are available, or ctx expires.
func (rl *RateLimiter) Wait(ctx context.Context) error {
	// Acquire rate-limit token
	select {
	case <-rl.tokens:
	case <-ctx.Done():
		return ctx.Err()
	}
	// Acquire inflight slot
	select {
	case rl.sem <- struct{}{}:
	case <-ctx.Done():
		// Return rate-limit token on inflight acquisition failure
		select {
		case rl.tokens <- struct{}{}:
		default:
		}
		return ctx.Err()
	}
	return nil
}

// Release frees an inflight slot.
func (rl *RateLimiter) Release() {
	select {
	case <-rl.sem:
	default:
	}
}

func (rl *RateLimiter) Stop() {
	close(rl.done)
}

// NewServer creates a new server instance
func NewServer() *Server {
	ctx, cancel := context.WithCancel(context.Background())
	return &Server{
		providerRegistry: providers.NewRegistry(),
		ctx:              ctx,
		cancel:           cancel,
		providerConfigs:  make(map[string]providers.ProviderConfig),
		rateLimiter:      NewRateLimiter(resolveRateLimits()),
		conns:            make(map[net.Conn]struct{}),
	}
}

// Start begins listening on the socket
func (s *Server) Start() error {
	if st, err := os.Lstat(SocketPath); err == nil {
		if st.Mode()&os.ModeSocket == 0 {
			return fmt.Errorf("refusing to remove non-socket path: %s", SocketPath)
		}
		os.Remove(SocketPath)
	}

	if err := os.MkdirAll(filepath.Dir(SocketPath), 0700); err != nil {
		return fmt.Errorf("failed to create socket directory: %w", err)
	}

	ln, err := net.Listen("unix", SocketPath)
	if err != nil {
		return fmt.Errorf("failed to create socket: %w", err)
	}
	s.listener = ln

	if err := os.Chmod(SocketPath, 0600); err != nil {
		ln.Close()
		return fmt.Errorf("failed to set socket permissions: %w", err)
	}

	log.Printf("API Gateway v%s listening on %s", Version, SocketPath)

	go s.acceptLoop()

	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)
	<-sigChan

	log.Println("Shutting down...")
	s.cancel()
	s.rateLimiter.Stop()
	ln.Close()
	// Force-close idle connections so wg.Wait does not block for 5 min
	s.connMu.Lock()
	for conn := range s.conns {
		conn.SetDeadline(time.Now())
	}
	s.connMu.Unlock()
	s.wg.Wait()
	return nil
}

func (s *Server) acceptLoop() {
	for {
		conn, err := s.listener.Accept()
		if err != nil {
			select {
			case <-s.ctx.Done():
				return
			default:
				log.Printf("Accept error: %v", err)
				continue
			}
		}
		s.wg.Add(1)
		go s.handleConnection(conn)
	}
}

func (s *Server) handleConnection(conn net.Conn) {
	defer s.wg.Done()
	defer func() {
		if r := recover(); r != nil {
			log.Printf("PANIC in handleConnection: %v\n%s", r, debug.Stack())
		}
	}()
	defer func() {
		s.connMu.Lock()
		delete(s.conns, conn)
		s.connMu.Unlock()
		conn.Close()
	}()
	s.connMu.Lock()
	s.conns[conn] = struct{}{}
	s.connMu.Unlock()

	// Cap request body at 10MB to prevent memory exhaustion from malicious clients.
	const maxRequestSize = 10 << 20
	decoder := json.NewDecoder(io.LimitReader(conn, maxRequestSize))
	encoder := json.NewEncoder(conn)
	w := &responseWriter{conn: conn, encoder: encoder}

	for {
		// Refresh read deadline for each message
		if err := conn.SetReadDeadline(time.Now().Add(5 * time.Minute)); err != nil {
			log.Printf("SetReadDeadline error: %v", err)
			return
		}

		var rawMsg json.RawMessage
		if err := decoder.Decode(&rawMsg); err != nil {
			return // EOF, timeout, or decode error — close connection
		}

		var typeCheck struct {
			Type string `json:"type"`
		}
		json.Unmarshal(rawMsg, &typeCheck)

		// Handle ping/pong
		if typeCheck.Type == "ping" {
			w.write(map[string]string{"type": "pong"})
			continue
		}

		// Handle set-provider
		if typeCheck.Type == "set-provider" {
			var setProviderReq SetProviderRequest
			if err := json.Unmarshal(rawMsg, &setProviderReq); err != nil {
				w.write(&Response{
					ID:     0,
					Status: 400,
					Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: fmt.Sprintf("Failed to parse set-provider request: %v", err)},
				})
				continue
			}

			s.configMu.Lock()
			s.providerConfigs[setProviderReq.Provider] = setProviderReq.Config
			log.Printf("Stored configuration for provider: %s", setProviderReq.Provider)
			s.configMu.Unlock()

			w.write(&Response{ID: 0, Status: 200})
			continue
		}

		// Handle list-providers
		if typeCheck.Type == "list-providers" {
			providerList := s.providerRegistry.SupportedProviders()
			body, _ := json.Marshal(map[string]interface{}{
				"providers": providerList,
			})
			w.write(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Handle provider-info
		if typeCheck.Type == "provider-info" {
			var infoReq struct {
				Provider string `json:"provider"`
			}
			if err := json.Unmarshal(rawMsg, &infoReq); err != nil || infoReq.Provider == "" {
				w.write(&Response{
					ID:     0,
					Status: 400,
					Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: "provider-info requires 'provider' field"},
				})
				continue
			}
			info := s.providerRegistry.GetCapabilities(infoReq.Provider)
			if info == nil {
				w.write(&Response{
					ID:     0,
					Status: 404,
					Error:  &ErrorDetail{Code: "NOT_FOUND", Message: "No capabilities for provider: " + infoReq.Provider},
				})
				continue
			}
			body, _ := json.Marshal(info)
			w.write(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Handle validate-parameters
		if typeCheck.Type == "validate-parameters" {
			var validateReq struct {
				Provider string                    `json:"provider"`
				Settings map[string]interface{}    `json:"settings"`
				Thinking *providers.ThinkingConfig `json:"thinking,omitempty"`
			}
			if err := json.Unmarshal(rawMsg, &validateReq); err != nil || validateReq.Provider == "" {
				w.write(&Response{
					ID:     0,
					Status: 400,
					Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: "validate-parameters requires 'provider' and 'settings' fields"},
				})
				continue
			}
			result := s.providerRegistry.ValidateSettings(validateReq.Provider, validateReq.Settings, validateReq.Thinking)
			if result == nil {
				w.write(&Response{
					ID:     0,
					Status: 200,
					Body:   json.RawMessage(`{"settings":{}}`),
				})
				continue
			}
			body, _ := json.Marshal(result)
			w.write(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Handle models (model discovery)
		if typeCheck.Type == "models" {
			var modelsReq struct {
				Provider string                  `json:"provider"`
				Config   providers.ProviderConfig `json:"config,omitempty"`
			}
			if err := json.Unmarshal(rawMsg, &modelsReq); err != nil || modelsReq.Provider == "" {
				w.write(&Response{
					ID:     0,
					Status: 400,
					Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: "models requires 'provider' field"},
				})
				continue
			}

			// Merge stored provider config
			cfg := modelsReq.Config
			cfg.ID = modelsReq.Provider
			s.configMu.RLock()
			if stored, ok := s.providerConfigs[modelsReq.Provider]; ok {
				if cfg.APIKey == "" && (cfg.BaseURL == "" || cfg.BaseURL == stored.BaseURL) {
					cfg.APIKey = stored.APIKey
				}
				if cfg.BaseURL == "" {
					cfg.BaseURL = stored.BaseURL
				}
			}
			s.configMu.RUnlock()

			modelsCtx, modelsCancel := context.WithTimeout(s.ctx, 30*time.Second)
				models, err := s.providerRegistry.ListModels(modelsCtx, modelsReq.Provider, cfg)
				modelsCancel()
			if err != nil {
				w.write(&Response{
					ID:     0,
					Status: 500,
					Error:  &ErrorDetail{Code: "FETCH_ERROR", Message: fmt.Sprintf("Failed to fetch models: %v", err)},
				})
				continue
			}
			if models == nil {
				w.write(&Response{
					ID:     0,
					Status: 200,
					Body:   json.RawMessage(`{"models":null}`),
				})
				continue
			}
			// Enrich with default capabilities from provider's ProviderInfo
			enrichModelCapabilities(models, s.providerRegistry.GetCapabilities(modelsReq.Provider), modelsReq.Provider)

			body, _ := json.Marshal(map[string]interface{}{"models": models})
			w.write(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Handle model-info (single model lookup)
		if typeCheck.Type == "model-info" {
			var modelInfoReq struct {
				Provider string                  `json:"provider"`
				Model    string                  `json:"model"`
				Config   providers.ProviderConfig `json:"config,omitempty"`
			}
			if err := json.Unmarshal(rawMsg, &modelInfoReq); err != nil || modelInfoReq.Provider == "" || modelInfoReq.Model == "" {
				w.write(&Response{
					ID:     0,
					Status: 400,
					Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: "model-info requires 'provider' and 'model' fields"},
				})
				continue
			}

			cfg := modelInfoReq.Config
			cfg.ID = modelInfoReq.Provider
			s.configMu.RLock()
			if stored, ok := s.providerConfigs[modelInfoReq.Provider]; ok {
				if cfg.APIKey == "" && (cfg.BaseURL == "" || cfg.BaseURL == stored.BaseURL) {
					cfg.APIKey = stored.APIKey
				}
				if cfg.BaseURL == "" {
					cfg.BaseURL = stored.BaseURL
				}
			}
			s.configMu.RUnlock()

			modelsCtx, modelsCancel := context.WithTimeout(s.ctx, 30*time.Second)
				models, err := s.providerRegistry.ListModels(modelsCtx, modelInfoReq.Provider, cfg)
				modelsCancel()
			if err != nil {
				w.write(&Response{
					ID:     0,
					Status: 500,
					Error:  &ErrorDetail{Code: "FETCH_ERROR", Message: fmt.Sprintf("Failed to fetch models: %v", err)},
				})
				continue
			}

			var found *providers.ModelEntry
			for i := range models {
				if models[i].ID == modelInfoReq.Model {
					found = &models[i]
					break
				}
			}
			if found == nil {
				w.write(&Response{
					ID:     0,
					Status: 404,
					Error:  &ErrorDetail{Code: "NOT_FOUND", Message: fmt.Sprintf("Model '%s' not found for provider '%s'", modelInfoReq.Model, modelInfoReq.Provider)},
				})
				continue
			}

			enriched := []providers.ModelEntry{*found}
			enrichModelCapabilities(enriched, s.providerRegistry.GetCapabilities(modelInfoReq.Provider), modelInfoReq.Provider)
			body, _ := json.Marshal(map[string]interface{}{"model": enriched[0]})
			w.write(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Handle codex-login (browser OAuth flow)
		if typeCheck.Type == "codex-login" {
			tokens, err := codexStartOAuth(s.ctx)
				if err != nil {
					body, _ := json.Marshal(map[string]interface{}{
						"type":    "codex-login-status",
						"status":  "error",
						"message": err.Error(),
					})
				w.write(&Response{ID: 0, Status: 200, Body: body})
				continue
			}
			if err := codexTokens.Save(tokens); err != nil {
					body, _ := json.Marshal(map[string]interface{}{
						"type":    "codex-login-status",
						"status":  "error",
						"message": fmt.Sprintf("save tokens: %v", err),
					})
				w.write(&Response{ID: 0, Status: 200, Body: body})
				continue
			}
			body, _ := json.Marshal(map[string]interface{}{
				"type":       "codex-login-status",
				"status":     "success",
				"account_id": tokens.AccountID,
			})
			w.write(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Handle codex-login-device (headless device code flow)
		if typeCheck.Type == "codex-login-device" {
			dc, err := codexStartDeviceCode(s.ctx)
			if err != nil {
				w.write(&Response{ID: 0, Status: 500, Error: &ErrorDetail{Code: "OAUTH_ERROR", Message: err.Error()}})
				continue
			}
			body, _ := json.Marshal(map[string]interface{}{
				"type":             "codex-login-device",
				"verification_url": dc.VerificationURL,
				"user_code":        dc.UserCode,
				"expires_at":       dc.ExpiresAt,
				"interval":         dc.Interval,
			})
			w.write(&Response{ID: 0, Status: 200, Body: body})

			// Poll synchronously to preserve JSON protocol ordering.
			tokens, err := codexPollDeviceCode(s.ctx, dc)
			if err != nil {
				body, _ = json.Marshal(map[string]interface{}{
					"type":    "codex-login-status",
					"status":  "error",
					"message": err.Error(),
				})
				w.write(&Response{ID: 0, Status: 200, Body: body})
				continue
			}
			if err := codexTokens.Save(tokens); err != nil {
				log.Printf("Warning: failed to save codex device tokens: %v", err)
			}
			body, _ = json.Marshal(map[string]interface{}{
				"type":       "codex-login-status",
				"status":     "success",
				"account_id": tokens.AccountID,
			})
			w.write(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

				// Handle codex-login-status (check if logged in)
		if typeCheck.Type == "codex-login-status" {
			tokens, err := codexTokens.Load()
			if err != nil {
				body, _ := json.Marshal(map[string]interface{}{
					"type":   "codex-login-status",
					"status": "not_authenticated",
				})
				w.write(&Response{ID: 0, Status: 200, Body: body})
				continue
			}
			body, _ := json.Marshal(map[string]interface{}{
				"type":       "codex-login-status",
				"status":     "authenticated",
				"account_id": tokens.AccountID,
			})
			w.write(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Regular API request
		var req Request
		if err := json.Unmarshal(rawMsg, &req); err != nil {
			w.write(&Response{
				ID:     0,
				Status: 400,
				Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: fmt.Sprintf("Failed to parse request: %v", err)},
			})
			continue
		}

		if req.Timeout == 0 {
			req.Timeout = 240000
		} else if req.Timeout > 3600000 {
			req.Timeout = 3600000 // cap at 1 hour to prevent Duration overflow
		}

		// Merge stored provider config (set-provider) into request
		s.mergeProviderConfig(&req)

		// Validate request after merging stored config
		if err := providers.ValidateRequest(s.buildProviderRequest(&req)); err != nil {
			w.write(&Response{
				ID:     req.ID,
				Status: 400,
				Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: err.Error()},
			})
			continue
		}

		ctx, cancel := context.WithTimeout(s.ctx, time.Duration(req.Timeout)*time.Millisecond)

		// Wait for rate limit token and inflight slot
		if err := s.rateLimiter.Wait(ctx); err != nil {
			w.write(&Response{
				ID:     req.ID,
				Status: 429,
				Error:  &ErrorDetail{Code: "RATE_LIMITED", Message: "Gateway queue timeout: too many concurrent requests", Retriable: true},
			})
			cancel()
			continue
		}

		func() {
			defer s.rateLimiter.Release()
			defer cancel()

			if req.Stream {
				// For streaming: send ack, then stream, then continue loop
				w.write(&Response{ID: req.ID, Status: 200})
				s.handleStreaming(ctx, req.ID, &req, w)
			} else {
				// For non-streaming: process and send response
				resp := s.processRequest(ctx, &req)
				w.write(resp)
			}
		}()
	}
}

// mergeProviderConfig fills in missing fields from the stored set-provider config.
// Precedence: request fields > stored config > handler defaults (resolved by the handler itself).
func (s *Server) mergeProviderConfig(req *Request) {
	if req.Provider.ID == "" {
		return
	}

	// For codex provider, inject OAuth token if no API key is set
	// This must run before the stored-config early return since openai_codex
	// never has a stored config (it uses OAuth, not API keys).
	if req.Provider.ID == "openai_codex" && req.Provider.APIKey == "" {
		if token, err := codexTokens.GetValidToken(); err == nil {
			req.Provider.APIKey = token
		}
	}

	s.configMu.RLock()
	stored, ok := s.providerConfigs[req.Provider.ID]
	s.configMu.RUnlock()
	if !ok {
		return
	}
	// Do not merge stored API key when the request overrides base_url to
	// a different host. This prevents key exfiltration via base_url redirect.
	overridesBaseURL := req.Provider.BaseURL != "" && req.Provider.BaseURL != stored.BaseURL
	if req.Provider.APIKey == "" && !overridesBaseURL {
		req.Provider.APIKey = stored.APIKey
	}
	if req.Provider.BaseURL == "" {
		req.Provider.BaseURL = stored.BaseURL
	}
	if req.Provider.Model == "" {
		req.Provider.Model = stored.Model
	}
	if req.Provider.Region == "" {
		req.Provider.Region = stored.Region
	}
	if req.Provider.ProjectID == "" {
		req.Provider.ProjectID = stored.ProjectID
	}
	if stored.Extra != nil {
		if req.Provider.Extra == nil {
			req.Provider.Extra = make(map[string]interface{}, len(stored.Extra))
			for k, v := range stored.Extra {
				req.Provider.Extra[k] = v
			}
		} else {
			for k, v := range stored.Extra {
				if _, exists := req.Provider.Extra[k]; !exists {
					req.Provider.Extra[k] = v
				}
			}
		}
	}

	// Validate the final base_url against SSRF, whether from request or stored config.
	if req.Provider.BaseURL != "" {
		if err := isSafeBaseURL(req.Provider.BaseURL); err != nil {
			log.Printf("[SSRF] rejected base_url %q: %v", req.Provider.BaseURL, err)
			req.Provider.BaseURL = "" // clear to prevent use
		}
	}
}

// enrichModelCapabilities fills in missing capability fields on ModelEntry
// using the provider's declared features as defaults. Per-model overrides
// (e.g., from a static table or API metadata) take precedence since we only
// set fields that are still at zero values.
func enrichModelCapabilities(models []providers.ModelEntry, info *providers.ProviderInfo, providerID string) {
	if info == nil {
		return
	}
	// Skip enrichment for providers that return per-model capability data.
	// Applying provider-wide defaults would overwrite accurate per-model info
	// (e.g. flipping text-only OpenRouter models to supports_images: true).
	switch providerID {
	case "openrouter":
		return
	}
	for i := range models {
		m := &models[i]
		if !m.SupportsImages && info.Features.SupportsImages {
			m.SupportsImages = true
		}
		if !m.SupportsPromptCache && info.Features.SupportsPromptCache {
			m.SupportsPromptCache = true
		}
		if !m.SupportsThinking && info.Features.SupportsThinking {
			m.SupportsThinking = true
		}
	}
}

func (s *Server) processRequest(ctx context.Context, req *Request) *Response {
	handler, err := s.providerRegistry.GetHandler(req.Provider.ID)
	if err != nil {
		return &Response{
			ID:     req.ID,
			Status: 400,
			Error:  &ErrorDetail{Code: "UNKNOWN_PROVIDER", Message: fmt.Sprintf("Provider '%s' not supported: %v", req.Provider.ID, err)},
		}
	}

	providerReq := s.buildProviderRequest(req)
	return s.handleNonStreaming(ctx, req.ID, handler, providerReq)
}

func (s *Server) buildProviderRequest(req *Request) *providers.Request {
	maxTokens := req.MaxTokens
	// User's per-role max_tokens setting overrides modelInfo default
	if mt, ok := req.Settings["max_tokens"]; ok {
		if v, ok := mt.(float64); ok && v > 0 {
			maxTokens = int(v)
		}
	}
	// If no max_tokens from caller or settings, use provider's default
	if maxTokens == 0 {
		if caps := s.providerRegistry.GetCapabilities(req.Provider.ID); caps != nil {
			if caps.MaxTokensDefault > 0 {
				maxTokens = caps.MaxTokensDefault
			}
		}
	}
	return &providers.Request{
		Provider:         req.Provider,
		Messages:         req.Messages,
		System:           req.System,
		Tools:            req.Tools,
		MaxTokens:        maxTokens,
		Temperature:      req.Temperature,
		TopP:             req.TopP,
		Stop:             req.Stop,
		ModelOverride:    req.ModelOverride,
		Thinking:         req.Thinking,
		Logprobs:         req.Logprobs,
		TopLogprobs:      req.TopLogprobs,
		PresencePenalty:  req.PresencePenalty,
		FrequencyPenalty: req.FrequencyPenalty,
		Settings:         req.Settings,
	}
}

func (s *Server) handleStreaming(ctx context.Context, id int64, req *Request, w *responseWriter) {
	handler, err := s.providerRegistry.GetHandler(req.Provider.ID)
	if err != nil {
		w.write(&Response{
			ID:     id,
			Status: 500,
			Error:  &ErrorDetail{Code: "PROVIDER_ERROR", Message: err.Error()},
		})
		return
	}

	providerReq := s.buildProviderRequest(req)

	// Child context: cancelled when handleStreaming returns, unblocking
	// the streaming goroutine even if parent ctx is still alive.
	streamCtx, streamCancel := context.WithCancel(ctx)
	defer streamCancel()

	chunks := make(chan providers.StreamChunk, 100)
	errChan := make(chan error, 1)
	doneChan := make(chan struct{}, 1)
	completeSent := false

	go func() {
		if streamErr := handler.Stream(streamCtx, providerReq, func(chunk providers.StreamChunk) error {
			select {
			case chunks <- chunk:
				return nil
			case <-streamCtx.Done():
				return streamCtx.Err()
			}
		}); streamErr != nil {
			errChan <- streamErr
		}
		doneChan <- struct{}{}
	}()

	for {
		select {
		case streamErr := <-errChan:
			w.write(&Response{
				ID:     id,
				Status: 500,
				Error:  &ErrorDetail{Code: "STREAM_ERROR", Message: sanitizeProviderError(streamErr), Retriable: providers.IsRetriable(streamErr)},
			})
			return
		case <-ctx.Done():
			streamCancel()
			select {
			case <-doneChan:
			case <-time.After(2 * time.Second):
			}
			w.write(&Response{
				ID:     id,
				Status: 499,
				Error:  &ErrorDetail{Code: "TIMEOUT", Message: "Request timed out"},
			})
			return
		case chunk := <-chunks:
			// Provider emitted complete -- forward it and stop (no duplicate)
			if chunk.Type == "complete" {
				completeSent = true
				w.write(&Response{ID: id, Status: 200, Body: mustMarshal(chunk)})
				return
			}
			if err := w.write(&Response{
				ID:     id,
				Status: 200,
				Body:   mustMarshal(chunk),
			}); err != nil {
				return // client disconnected, cancel via deferred streamCancel
			}
		case <-doneChan:
			// Check for error that arrived simultaneously with done signal
			select {
			case streamErr := <-errChan:
				w.write(&Response{
					ID:     id,
					Status: 500,
					Error:  &ErrorDetail{Code: "STREAM_ERROR", Message: sanitizeProviderError(streamErr), Retriable: providers.IsRetriable(streamErr)},
				})
				return
			default:
			}
			// Drain remaining chunks before closing
			for {
				select {
				case chunk := <-chunks:
					if chunk.Type == "complete" {
						completeSent = true
						w.write(&Response{ID: id, Status: 200, Body: mustMarshal(chunk)})
						return
					}
					w.write(&Response{
						ID:     id,
						Status: 200,
						Body:   mustMarshal(chunk),
					})
				default:
					// Final check: an error may have arrived between the first check and now
					select {
					case streamErr := <-errChan:
						w.write(&Response{
							ID:     id,
							Status: 500,
							Error:  &ErrorDetail{Code: "STREAM_ERROR", Message: sanitizeProviderError(streamErr), Retriable: providers.IsRetriable(streamErr)},
						})
						return
					default:
					}
					// Send complete only if provider did not emit one
					if !completeSent {
						w.write(&Response{
							ID:     id,
							Status: 200,
							Body:   json.RawMessage(`{"type":"complete"}`),
						})
					}
					return
				}
			}
		}
	}
}

func (s *Server) handleNonStreaming(ctx context.Context, id int64, handler providers.Handler, req *providers.Request) *Response {
	const maxAttempts = 9 // 1 initial + 8 retries
	var lastErr error
	var lastRetriable bool

	for attempt := 0; attempt < maxAttempts; attempt++ {
		if attempt > 0 {
			backoff := time.Duration(1<<uint(attempt-1)) * time.Second
			if backoff > 60*time.Second {
				backoff = 60 * time.Second
			}
			log.Printf("Retry attempt %d/%d after %v", attempt, maxAttempts-1, backoff)

			timer := time.NewTimer(backoff)
			select {
			case <-timer.C:
			case <-ctx.Done():
				timer.Stop()
				return &Response{
					ID:     id,
					Status: 499,
					Error:  &ErrorDetail{Code: "TIMEOUT", Message: "Request timed out during retry backoff"},
				}
			}
		}

		result, err := handler.Send(ctx, req)
		if err == nil {
			body, err := json.Marshal(result)
			if err != nil {
				return &Response{
					ID:     id,
					Status: 500,
					Error:  &ErrorDetail{Code: "MARSHAL_ERROR", Message: err.Error()},
				}
			}
			return &Response{
				ID:     id,
				Status: 200,
				Body:   body,
			}
		}

		lastErr = err
		lastRetriable = providers.IsRetriable(err)

		if !lastRetriable {
			log.Printf("Non-retriable error, giving up: %v", err)
			break
		}
	}

	return &Response{
		ID:     id,
		Status: 500,
		Error: &ErrorDetail{
			Code:      "PROVIDER_ERROR",
			Message:   sanitizeProviderError(lastErr),
			Retriable: lastRetriable,
		},
	}
}

// isSafeBaseURL rejects URLs that point to private/internal networks (SSRF protection).
// Allows localhost/127.0.0.1 for local providers (Ollama, LM Studio).
func isSafeBaseURL(rawURL string) error {
	u, err := url.Parse(rawURL)
	if err != nil {
		return fmt.Errorf("invalid URL: %w", err)
	}
	host := u.Hostname()
	if host == "" {
		return nil // relative URL, no host
	}

	// Allow localhost variants for local providers
	if host == "localhost" || host == "127.0.0.1" || host == "::1" {
		return nil
	}

	// Resolve and check for private/link-local/metadata IPs
	ips, err := net.LookupIP(host)
	if err != nil {
		// DNS resolution failed — allow through so users can use hostnames
		// that may not resolve on the gateway host but resolve in the client's network.
		// The actual request will fail if the host is truly unreachable.
		return nil
	}
	for _, ip := range ips {
		addr, ok := netip.AddrFromSlice(ip)
		if !ok {
			continue
		}
		if addr.IsPrivate() || addr.IsLinkLocalUnicast() || addr.IsLoopback() {
			return fmt.Errorf("base_url host %s resolves to private/internal IP %s", host, addr)
		}

	}
	return nil
}

// sanitizeProviderError strips raw response bodies from provider errors
// before forwarding to the client, preventing internal detail leaks.
func sanitizeProviderError(err error) string {
	if pae, ok := err.(*providers.ProviderAPIError); ok {
		return fmt.Sprintf("provider returned status %d", pae.StatusCode)
	}
	// For non-API errors (I/O timeouts, DNS, etc.), return only a generic
	// category to avoid leaking internal hostnames, IPs, or paths.
	msg := err.Error()
	switch {
	case strings.Contains(msg, "timeout") || strings.Contains(msg, "deadline"):
		return "provider request timed out"
	case strings.Contains(msg, "connection refused"):
		return "provider connection refused"
	case strings.Contains(msg, "DNS") || strings.Contains(msg, "lookup"):
		return "provider DNS resolution failed"
	case strings.Contains(msg, "TLS") || strings.Contains(msg, "certificate"):
		return "provider TLS error"
	default:
		return "provider request failed"
	}
}

func mustMarshal(v interface{}) json.RawMessage {
	data, err := json.Marshal(v)
	if err != nil {
		log.Printf("mustMarshal error: %v", err)
		return json.RawMessage(`{"type":"error","error":"internal marshal error"}`)
	}
	return data
}

func main() {
	server := NewServer()
	if err := server.Start(); err != nil {
		log.Fatalf("Server error: %v", err)
	}
}
