package main

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log"
	mrand "math/rand/v2"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

const (
	codexClientIDDefault = "app_EMoamEEZ73f0CkXaXp7hrann"
	codexAuthURL         = "https://auth.openai.com/oauth/authorize"
	codexTokenURL        = "https://auth.openai.com/oauth/token"
	codexCallbackPort    = 1455
	codexScope           = "openid profile email offline_access"
)

// codexClientID returns the OAuth client ID, overridable via CODEX_CLIENT_ID env var.
func codexClientID() string {
	if v := os.Getenv("CODEX_CLIENT_ID"); v != "" {
		return v
	}
	return codexClientIDDefault
}

// CodexAuthTokens holds the OAuth tokens for Codex authentication.
type CodexAuthTokens struct {
	AccessToken  string `json:"access_token"`
	RefreshToken string `json:"refresh_token"`
	IDToken      string `json:"id_token"`
	AccountID    string `json:"account_id"`
	Expiry       int64  `json:"expiry"`
	LastRefresh  string `json:"last_refresh"`
}

// codexTokenStore manages Codex OAuth tokens with file-based persistence.
type codexTokenStore struct {
	mu                sync.RWMutex
	path              string
	lastRefreshErr    error
	lastRefreshAttempt time.Time
}

var codexTokens = &codexTokenStore{}

// oauthHTTPClient is a shared client for all OAuth token exchange/refresh calls.
// These are short-lived request/response calls (not SSE streams), so a timeout is safe.
var oauthHTTPClient = &http.Client{Timeout: 30 * time.Second}

func init() {
	home, err := os.UserHomeDir()
	if err != nil {
		return
	}
	dir := filepath.Join(home, ".di")
	os.MkdirAll(dir, 0700)
	codexTokens.path = filepath.Join(dir, "codex-auth.json")
}

// loadFromFile reads tokens from disk. Caller must hold appropriate lock.
func (s *codexTokenStore) loadFromFile() (*CodexAuthTokens, error) {
	if s.path == "" {
		return nil, fmt.Errorf("token path not set")
	}
	data, err := os.ReadFile(s.path)
	if err != nil {
		return nil, err
	}
	var tokens CodexAuthTokens
	if err := json.Unmarshal(data, &tokens); err != nil {
		return nil, err
	}
	return &tokens, nil
}

// saveToFile writes tokens to disk atomically via temp file + rename.
// Caller must hold Lock.
func (s *codexTokenStore) saveToFile(tokens *CodexAuthTokens) error {
	if s.path == "" {
		return fmt.Errorf("token path not set")
	}
	data, err := json.MarshalIndent(tokens, "", "  ")
	if err != nil {
		return err
	}
	tmp := s.path + ".tmp"
	f, err := os.OpenFile(tmp, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0600)
	if err != nil {
		return err
	}
	if _, err := f.Write(data); err != nil {
		f.Close()
		os.Remove(tmp)
		return err
	}
	if err := f.Sync(); err != nil {
		f.Close()
		os.Remove(tmp)
		return err
	}
	f.Close()
	if err := os.Rename(tmp, s.path); err != nil {
		os.Remove(tmp)
		return err
	}
	return nil
}

// Load reads tokens from disk (thread-safe).
func (s *codexTokenStore) Load() (*CodexAuthTokens, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.loadFromFile()
}

// Save writes tokens to disk with restricted permissions (thread-safe).
func (s *codexTokenStore) Save(tokens *CodexAuthTokens) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.saveToFile(tokens)
}

// GetValidToken returns a valid access token, refreshing if needed.
func (s *codexTokenStore) GetValidToken() (string, error) {
	tokens, err := s.Load()
	if err != nil {
		return "", fmt.Errorf("no stored codex tokens: %w", err)
	}

	// If token is still valid for 5+ minutes, use it
	if time.Until(time.Unix(tokens.Expiry, 0)) > 5*time.Minute {
		return tokens.AccessToken, nil
	}

	// Acquire write lock to serialize refresh (prevents thundering herd).
	s.mu.Lock()
	defer s.mu.Unlock()

	// Double-check: another goroutine may have refreshed while we waited.
	tokens, err = s.loadFromFile()
	if err != nil {
		return "", fmt.Errorf("no stored codex tokens: %w", err)
	}
	if time.Until(time.Unix(tokens.Expiry, 0)) > 5*time.Minute {
		return tokens.AccessToken, nil
	}

	newTokens, err := codexRefreshToken(tokens.RefreshToken)
	if err != nil {
		return "", fmt.Errorf("token refresh failed: %w", err)
	}

	if err := s.saveToFile(newTokens); err != nil {
		log.Printf("Warning: failed to save refreshed codex token: %v", err)
	}

	return newTokens.AccessToken, nil
}

// --- PKCE helpers ---

func generateCodeVerifier() (string, error) {
	b := make([]byte, 64)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return base64.URLEncoding.WithPadding(base64.NoPadding).EncodeToString(b), nil
}

func generateCodeChallenge(verifier string) string {
	hash := sha256.Sum256([]byte(verifier))
	return base64.URLEncoding.WithPadding(base64.NoPadding).EncodeToString(hash[:])
}

// --- Browser OAuth flow ---

// codexStartOAuth starts the browser-based OAuth flow.
// Returns tokens on success.
func codexStartOAuth(ctx context.Context) (*CodexAuthTokens, error) {
	verifier, err := generateCodeVerifier()
	if err != nil {
		return nil, fmt.Errorf("generate PKCE verifier: %w", err)
	}
	challenge := generateCodeChallenge(verifier)

	// Find an available port starting from the default
	port := codexCallbackPort
	var listener net.Listener
	for attempt := 0; attempt < 10; attempt++ {
		listener, err = net.Listen("tcp", fmt.Sprintf("localhost:%d", port))
		if err == nil {
			break
		}
		port++
	}
	if listener == nil {
		return nil, fmt.Errorf("could not bind callback port after 10 attempts")
	}
	defer listener.Close()

	redirectURI := fmt.Sprintf("http://localhost:%d/auth/callback", port)

	// Build authorization URL
	authURL, _ := url.Parse(codexAuthURL)
	q := authURL.Query()
	q.Set("response_type", "code")
	q.Set("client_id", codexClientID())
	q.Set("redirect_uri", redirectURI)
	q.Set("scope", codexScope)
	q.Set("code_challenge", challenge)
	q.Set("code_challenge_method", "S256")
	state, err := randomState()
	if err != nil {
		return nil, err
	}
	q.Set("state", state)
	q.Set("codex_cli_simplified_flow", "true")
	authURL.RawQuery = q.Encode()

	// Channel to receive the auth code
	codeChan := make(chan string, 1)
	errChan := make(chan error, 1)

	// Start HTTP server for callback (shutdown via defer ensures cleanup on all paths).
	srv := &http.Server{
		ReadTimeout:  5 * time.Second,
		WriteTimeout: 10 * time.Second,
		IdleTimeout:  15 * time.Second,
	}
	defer srv.Shutdown(context.Background())
	srv.Handler = http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		log.Printf("[codex-oauth] Callback request: %s %s", r.Method, r.URL.String())

		if r.URL.Path != "/auth/callback" {
			return
		}

		// Check for OAuth error response
		if errParam := r.URL.Query().Get("error"); errParam != "" {
			errDesc := r.URL.Query().Get("error_description")
			errChan <- fmt.Errorf("OAuth error: %s: %s", errParam, errDesc)
			w.WriteHeader(400)
			w.Write([]byte(fmt.Sprintf("Error: %s", errDesc)))
			return
		}

		// Verify state to prevent CSRF
		if callbackState := r.URL.Query().Get("state"); callbackState != state {
			errChan <- fmt.Errorf("OAuth state mismatch")
			w.WriteHeader(400)
			w.Write([]byte("Error: invalid state parameter"))
			return
		}

		code := r.URL.Query().Get("code")
		if code == "" {
			errChan <- fmt.Errorf("no authorization code in callback")
			w.WriteHeader(400)
			w.Write([]byte("Error: missing authorization code"))
			return
		}
		codeChan <- code
		w.Write([]byte("Authentication successful! You can close this tab."))
	})

	go srv.Serve(listener)

	// Open browser
	if err := openBrowser(authURL.String()); err != nil {
		return nil, fmt.Errorf("failed to open browser: %w", err)
	}

	log.Printf("Waiting for Codex OAuth callback on port %d...", port)

	// Wait for callback, context cancellation, or 5-minute timeout
	timeoutCtx, timeoutCancel := context.WithTimeout(ctx, 5*time.Minute)
	defer timeoutCancel()
	select {
	case code := <-codeChan:
		return codexExchangeCode(code, verifier, redirectURI)
	case err := <-errChan:
		return nil, err
	case <-timeoutCtx.Done():
		return nil, fmt.Errorf("authentication timed out: %w", timeoutCtx.Err())
	}
}

// codexExchangeCode exchanges an authorization code for tokens.
func codexExchangeCode(code, verifier, redirectURI string) (*CodexAuthTokens, error) {
	// Step 1: Exchange code for tokens
	form := url.Values{
		"grant_type":    {"authorization_code"},
		"code":          {code},
		"redirect_uri":  {redirectURI},
		"client_id":     {codexClientID()},
		"code_verifier": {verifier},
	}

	resp, err := oauthHTTPClient.PostForm(codexTokenURL, form)
	if err != nil {
		return nil, fmt.Errorf("token exchange request: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, fmt.Errorf("read token response: %w", err)
	}

	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("token exchange failed (status %d): %s", resp.StatusCode, string(body))
	}

	var tokenResp struct {
		AccessToken  string `json:"access_token"`
		RefreshToken string `json:"refresh_token"`
		IDToken      string `json:"id_token"`
		ExpiresIn    int    `json:"expires_in"`
	}
	if err := json.Unmarshal(body, &tokenResp); err != nil {
		return nil, fmt.Errorf("parse token response: %w", err)
	}

	// Step 2: Exchange id_token for API access token (token exchange grant)
	apiToken, accountID, err := codexExchangeForAPIToken(tokenResp.IDToken)
	if err != nil {
		return nil, fmt.Errorf("API token exchange failed (re-authentication required): %w", err)
	}

	return &CodexAuthTokens{
		AccessToken:  apiToken,
		RefreshToken: tokenResp.RefreshToken,
		IDToken:      tokenResp.IDToken,
		AccountID:    accountID,
		Expiry:       time.Now().Add(time.Duration(tokenResp.ExpiresIn) * time.Second).Unix(),
		LastRefresh:  time.Now().UTC().Format(time.RFC3339),
	}, nil
}

// codexExchangeForAPIToken trades the id_token for an API access token.
func codexExchangeForAPIToken(idToken string) (accessToken, accountID string, err error) {
	form := url.Values{
		"grant_type":           {"urn:ietf:params:oauth:grant-type:token-exchange"},
		"client_id":            {codexClientID()},
		"requested_token":      {"openai-api-key"},
		"subject_token":        {idToken},
		"subject_token_type":   {"urn:ietf:params:oauth:token-type:id_token"},
	}

	resp, err := oauthHTTPClient.PostForm(codexTokenURL, form)
	if err != nil {
		return "", "", fmt.Errorf("API token exchange request: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return "", "", fmt.Errorf("read API token response: %w", err)
	}

	if resp.StatusCode != 200 {
		return "", "", fmt.Errorf("API token exchange failed (status %d): %s", resp.StatusCode, string(body))
	}

	var result struct {
		AccessToken string `json:"access_token"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		return "", "", fmt.Errorf("parse API token response: %w", err)
	}

	return result.AccessToken, extractAccountID(idToken), nil
}

// codexRefreshToken refreshes an expired access token.
func codexRefreshToken(refreshToken string) (*CodexAuthTokens, error) {
	form := url.Values{
		"client_id":     {codexClientID()},
		"grant_type":    {"refresh_token"},
		"refresh_token": {refreshToken},
		"scope":         {"openid profile email offline_access"},
	}

	resp, err := oauthHTTPClient.PostForm(codexTokenURL, form)
	if err != nil {
		return nil, fmt.Errorf("refresh request: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, fmt.Errorf("read refresh response: %w", err)
	}

	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("refresh failed (status %d): %s", resp.StatusCode, string(body))
	}

	var tokenResp struct {
		AccessToken  string `json:"access_token"`
		RefreshToken string `json:"refresh_token"`
		IDToken      string `json:"id_token"`
		ExpiresIn    int    `json:"expires_in"`
	}
	if err := json.Unmarshal(body, &tokenResp); err != nil {
		return nil, fmt.Errorf("parse refresh response: %w", err)
	}

	// Also refresh the API token
	apiToken, accountID, err := codexExchangeForAPIToken(tokenResp.IDToken)
	if err != nil {
		return nil, fmt.Errorf("API token exchange on refresh failed (re-authentication required): %w", err)
	}

	newRefresh := tokenResp.RefreshToken
	if newRefresh == "" {
		newRefresh = refreshToken
	}

	return &CodexAuthTokens{
		AccessToken:  apiToken,
		RefreshToken: newRefresh,
		IDToken:      tokenResp.IDToken,
		AccountID:    accountID,
		Expiry:       time.Now().Add(time.Duration(tokenResp.ExpiresIn) * time.Second).Unix(),
		LastRefresh:  time.Now().UTC().Format(time.RFC3339),
	}, nil
}

// --- Device code flow (headless alternative) ---

// CodexDeviceCode holds the device code flow state.
type CodexDeviceCode struct {
	VerificationURL string `json:"verification_url"`
	UserCode        string `json:"user_code"`
	ExpiresAt       int64  `json:"expires_at"`
	Interval        int    `json:"interval"`
	deviceAuthID    string
	codeVerifier    string
}

// codexStartDeviceCode initiates the device code flow.
func codexStartDeviceCode(ctx context.Context) (*CodexDeviceCode, error) {
	verifier, err := generateCodeVerifier()
	if err != nil {
		return nil, err
	}

	payload, _ := json.Marshal(map[string]string{
		"client_id": codexClientID(),
	})

	resp, err := oauthHTTPClient.Post("https://auth.openai.com/api/accounts/deviceauth/usercode", "application/json", strings.NewReader(string(payload)))
	if err != nil {
		return nil, fmt.Errorf("device code request: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, fmt.Errorf("read device code response: %w", err)
	}

	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("device code failed (status %d): %s", resp.StatusCode, string(body))
	}

	var result struct {
		DeviceAuthID string `json:"device_auth_id"`
		UserCode     string `json:"user_code"`
		Interval     int    `json:"interval"`
		ExpiresIn    int    `json:"expires_in"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, fmt.Errorf("parse device code response: %w", err)
	}

	expiresIn := 900 // 15 minutes default
	if result.ExpiresIn > 0 {
		expiresIn = result.ExpiresIn
	}

	return &CodexDeviceCode{
		VerificationURL: "https://auth.openai.com/codex/device",
		UserCode:        result.UserCode,
		ExpiresAt:       time.Now().Add(time.Duration(expiresIn) * time.Second).Unix(),
		Interval:        result.Interval,
		deviceAuthID:    result.DeviceAuthID,
		codeVerifier:    verifier,
	}, nil
}

// codexPollDeviceCode polls for device code completion.
func codexPollDeviceCode(ctx context.Context, dc *CodexDeviceCode) (*CodexAuthTokens, error) {
	interval := time.Duration(dc.Interval) * time.Second
	backoff := time.Duration(0)
	// Add jitter to avoid thundering herd when multiple pollers share the same interval.
	jitterMax := time.Second
	if interval > 4*time.Second {
		jitterMax = interval / 4
	}

	for {
		wait := interval + backoff + time.Duration(mrand.Int64N(int64(jitterMax)))
		t := time.NewTimer(wait)
		select {
		case <-ctx.Done():
			t.Stop()
			return nil, ctx.Err()
		case <-t.C:
		}

		if time.Now().Unix() > dc.ExpiresAt {
			return nil, fmt.Errorf("device code expired")
		}

		payload, _ := json.Marshal(map[string]string{
			"device_auth_id": dc.deviceAuthID,
			"user_code":      dc.UserCode,
		})

		pollReq, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://auth.openai.com/api/accounts/deviceauth/token", strings.NewReader(string(payload)))
		if err != nil {
			return nil, fmt.Errorf("create device token request: %w", err)
		}
		pollReq.Header.Set("Content-Type", "application/json")
		resp, err := oauthHTTPClient.Do(pollReq)
		if err != nil {
			backoff = min(backoff+time.Second, 30*time.Second)
			continue
		}
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
		resp.Body.Close()

		if resp.StatusCode == 200 {
			var result struct {
				AuthorizationCode string `json:"authorization_code"`
				CodeChallenge      string `json:"code_challenge"`
				CodeVerifier       string `json:"code_verifier"`
			}
			if err := json.Unmarshal(body, &result); err != nil {
				return nil, fmt.Errorf("parse device token response: %w", err)
			}

			verifier := result.CodeVerifier
			if verifier == "" {
				verifier = dc.codeVerifier
			}
			return codexExchangeCode(result.AuthorizationCode, verifier, "")
		}

		// 5xx: back off to avoid hammering a struggling server
		if resp.StatusCode >= 500 {
			backoff = min(backoff+2*time.Second, 30*time.Second)
			continue
		}
		// 4xx/other: still pending, reset backoff
		backoff = 0
	}
}

// --- Helpers ---

func randomState() (string, error) {
	b := make([]byte, 32)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("crypto/rand.Read failed: %w", err)
	}
	return base64.URLEncoding.WithPadding(base64.NoPadding).EncodeToString(b), nil
}

func openBrowser(url string) error {
	var cmd string
	var args []string

	switch {
	case commandExists("xdg-open"):
		cmd = "xdg-open"
		args = []string{url}
	case commandExists("open"):
		cmd = "open"
		args = []string{url}
	case commandExists("sensible-browser"):
		cmd = "sensible-browser"
		args = []string{url}
	default:
		return fmt.Errorf("no browser command found (tried xdg-open, open, sensible-browser)")
	}

	c := exec.Command(cmd, args...)
	if err := c.Start(); err != nil {
		return err
	}
	go c.Wait() // reap the child process to prevent zombies
	return nil
}

func commandExists(name string) bool {
	_, err := exec.LookPath(name)
	return err == nil
}

// extractAccountID tries to extract the account_id from a JWT id_token payload.
func extractAccountID(idToken string) string {
	parts := strings.Split(idToken, ".")
	if len(parts) < 2 {
		return ""
	}
	payload, err := base64.URLEncoding.WithPadding(base64.NoPadding).DecodeString(parts[1])
	if err != nil {
		// Try standard base64
		payload, err = base64.StdEncoding.DecodeString(parts[1])
		if err != nil {
			return ""
		}
	}
	var claims struct {
		AccountID string `json:"https://api.openai.com/auth|account_id"`
		Sub       string `json:"sub"`
	}
	if json.Unmarshal(payload, &claims) == nil && claims.AccountID != "" {
		return claims.AccountID
	}
	return ""
}
