package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net"
	"os"
	"os/signal"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/dirac-dev/api-gateway/providers"
)

const Version = "0.1.0"

// Build-time configurable rate limits (override via -ldflags "-X main.maxRequestsPerSec=10 -X main.maxInflightReqs=5")
var maxRequestsPerSec int = 5
var maxInflightReqs int = 3

var SocketPath = os.Getenv("DIRAC_API_GATEWAY_SOCKET")

func init() {
	home, _ := os.UserHomeDir()
	if SocketPath == "" {
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
}

// RateLimiter controls outbound request rate and concurrency.
// Token bucket for rate limiting, buffered channel for max-inflight.
type RateLimiter struct {
	tokens chan struct{} // token bucket for req/s rate limiting
	sem    chan struct{} // semaphore for max concurrent requests
	done   chan struct{}
}

func NewRateLimiter(ratePerSec, maxConcurrent int) *RateLimiter {
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
		rateLimiter:      NewRateLimiter(maxRequestsPerSec, maxInflightReqs),
	}
}

// Start begins listening on the socket
func (s *Server) Start() error {
	os.Remove(SocketPath)

	ln, err := net.Listen("unix", SocketPath)
	if err != nil {
		return fmt.Errorf("failed to create socket: %w", err)
	}
	s.listener = ln

	os.Chmod(SocketPath, 0600)

	log.Printf("API Gateway v%s listening on %s", Version, SocketPath)

	go s.acceptLoop()

	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)
	<-sigChan

	log.Println("Shutting down...")
	s.cancel()
	s.rateLimiter.Stop()
	ln.Close()
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
	defer conn.Close()

	decoder := json.NewDecoder(conn)
	encoder := json.NewEncoder(conn)

	for {
		// Refresh read deadline for each message
		conn.SetReadDeadline(time.Now().Add(5 * time.Minute))

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
			encoder.Encode(map[string]string{"type": "pong"})
			continue
		}

		// Handle set-provider
		if typeCheck.Type == "set-provider" {
			var setProviderReq SetProviderRequest
			if err := json.Unmarshal(rawMsg, &setProviderReq); err != nil {
				encoder.Encode(&Response{
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

			encoder.Encode(&Response{ID: 0, Status: 200})
			continue
		}

		// Handle list-providers
		if typeCheck.Type == "list-providers" {
			providerList := s.providerRegistry.SupportedProviders()
			body, _ := json.Marshal(map[string]interface{}{
				"providers": providerList,
			})
			encoder.Encode(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Handle provider-info
		if typeCheck.Type == "provider-info" {
			var infoReq struct {
				Provider string `json:"provider"`
			}
			if err := json.Unmarshal(rawMsg, &infoReq); err != nil || infoReq.Provider == "" {
				encoder.Encode(&Response{
					ID:     0,
					Status: 400,
					Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: "provider-info requires 'provider' field"},
				})
				continue
			}
			info := s.providerRegistry.GetCapabilities(infoReq.Provider)
			if info == nil {
				encoder.Encode(&Response{
					ID:     0,
					Status: 404,
					Error:  &ErrorDetail{Code: "NOT_FOUND", Message: "No capabilities for provider: " + infoReq.Provider},
				})
				continue
			}
			body, _ := json.Marshal(info)
			encoder.Encode(&Response{ID: 0, Status: 200, Body: body})
			continue
		}

		// Regular API request
		var req Request
		if err := json.Unmarshal(rawMsg, &req); err != nil {
			encoder.Encode(&Response{
				ID:     0,
				Status: 400,
				Error:  &ErrorDetail{Code: "INVALID_REQUEST", Message: fmt.Sprintf("Failed to parse request: %v", err)},
			})
			continue
		}

		if req.Timeout == 0 {
			req.Timeout = 240000
		}

		// Merge stored provider config (set-provider) into request
		s.mergeProviderConfig(&req)

		ctx, cancel := context.WithTimeout(s.ctx, time.Duration(req.Timeout)*time.Millisecond)

		// Wait for rate limit token and inflight slot
		if err := s.rateLimiter.Wait(ctx); err != nil {
			encoder.Encode(&Response{
				ID:     req.ID,
				Status: 429,
				Error:  &ErrorDetail{Code: "RATE_LIMITED", Message: "Gateway queue timeout: too many concurrent requests", Retriable: true},
			})
			cancel()
			continue
		}

		if req.Stream {
			// For streaming: send ack, then stream, then continue loop
			encoder.Encode(&Response{ID: req.ID, Status: 200})
			s.handleStreaming(ctx, req.ID, &req, encoder)
			s.rateLimiter.Release()
			cancel()
		} else {
			// For non-streaming: process and send response
			resp := s.processRequest(ctx, &req)
			s.rateLimiter.Release()
			encoder.Encode(resp)
			cancel()
		}
	}
}

// mergeProviderConfig fills in missing provider config from stored set-provider configs
func (s *Server) mergeProviderConfig(req *Request) {
	if req.Provider.ID == "" {
		return
	}
	s.configMu.RLock()
	stored, ok := s.providerConfigs[req.Provider.ID]
	s.configMu.RUnlock()
	if !ok {
		return
	}
	if req.Provider.APIKey == "" {
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
	return &providers.Request{
		Provider:         req.Provider,
		Messages:         req.Messages,
		System:           req.System,
		Tools:            req.Tools,
		MaxTokens:        req.MaxTokens,
		Temperature:      req.Temperature,
		TopP:             req.TopP,
		Stop:             req.Stop,
		ModelOverride:    req.ModelOverride,
		Thinking:         req.Thinking,
		Logprobs:         req.Logprobs,
		TopLogprobs:      req.TopLogprobs,
		PresencePenalty:  req.PresencePenalty,
		FrequencyPenalty: req.FrequencyPenalty,
	}
}

func (s *Server) handleStreaming(ctx context.Context, id int64, req *Request, encoder *json.Encoder) {
	handler, err := s.providerRegistry.GetHandler(req.Provider.ID)
	if err != nil {
		encoder.Encode(&Response{
			ID:     id,
			Status: 500,
			Error:  &ErrorDetail{Code: "PROVIDER_ERROR", Message: err.Error()},
		})
		return
	}

	providerReq := s.buildProviderRequest(req)

	chunks := make(chan providers.StreamChunk, 100)
	errChan := make(chan error, 1)
	doneChan := make(chan struct{})

	go func() {
		if streamErr := handler.Stream(ctx, providerReq, func(chunk providers.StreamChunk) error {
			select {
			case chunks <- chunk:
				return nil
			case <-ctx.Done():
				return ctx.Err()
			}
		}); streamErr != nil {
			errChan <- streamErr
		}
		doneChan <- struct{}{}
	}()

	for {
		select {
		case streamErr := <-errChan:
			encoder.Encode(&Response{
				ID:     id,
				Status: 500,
				Error:  &ErrorDetail{Code: "STREAM_ERROR", Message: streamErr.Error()},
			})
			return
		case <-ctx.Done():
			encoder.Encode(&Response{
				ID:     id,
				Status: 499,
				Error:  &ErrorDetail{Code: "TIMEOUT", Message: "Request timed out"},
			})
			return
		case chunk := <-chunks:
			encoder.Encode(&Response{
				ID:     id,
				Status: 200,
				Body:   mustMarshal(chunk),
			})
		case <-doneChan:
			// Check for error that arrived simultaneously with done signal
			select {
			case streamErr := <-errChan:
				encoder.Encode(&Response{
					ID:     id,
					Status: 500,
					Error:  &ErrorDetail{Code: "STREAM_ERROR", Message: streamErr.Error()},
				})
				return
			default:
			}
			// Drain remaining chunks before closing
			for {
				select {
				case chunk := <-chunks:
					encoder.Encode(&Response{
						ID:     id,
						Status: 200,
						Body:   mustMarshal(chunk),
					})
				default:
					// Always send complete signal so the client never hangs
					encoder.Encode(&Response{
						ID:     id,
						Status: 200,
						Body:   json.RawMessage(`{"type":"complete"}`),
					})
					return
				}
			}
		}
	}
}

func (s *Server) handleNonStreaming(ctx context.Context, id int64, handler providers.Handler, req *providers.Request) *Response {
	maxRetries := 3
	var lastErr error

	for attempt := 0; attempt <= maxRetries; attempt++ {
		if attempt > 0 {
			backoff := time.Duration(1<<uint(attempt-1)) * time.Second
			log.Printf("Retry attempt %d/%d after %v", attempt, maxRetries, backoff)

			select {
			case <-time.After(backoff):
			case <-ctx.Done():
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

		if !isRetriableError(err) {
			log.Printf("Non-retriable error, giving up: %v", err)
			break
		}
	}

	return &Response{
		ID:     id,
		Status: 500,
		Error: &ErrorDetail{
			Code:      "PROVIDER_ERROR",
			Message:   lastErr.Error(),
			Retriable: true,
		},
	}
}

func isRetriableError(err error) bool {
	msg := err.Error()
	if strings.Contains(msg, "429") || strings.Contains(msg, "rate_limit") || strings.Contains(msg, "rate limit") {
		return true
	}
	if strings.Contains(msg, "500") || strings.Contains(msg, "502") || strings.Contains(msg, "503") || strings.Contains(msg, "504") {
		return true
	}
	return false
}

func mustMarshal(v interface{}) json.RawMessage {
	data, err := json.Marshal(v)
	if err != nil {
		return json.RawMessage(`{}`)
	}
	return data
}

func main() {
	server := NewServer()
	if err := server.Start(); err != nil {
		log.Fatalf("Server error: %v", err)
	}
}
