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
	codexClientID    = "app_EMoamEEZ73f0CkXaXp7hrann"
	codexAuthURL     = "https://auth.openai.com/oauth/authorize"
	codexTokenURL    = "https://auth.openai.com/oauth/token"
	codexCallbackPort = 1455
	codexScope       = "openid profile email offline_access"
)

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
	mu   sync.RWMutex
	path string
}

var codexTokens = &codexTokenStore{}

func init() {
	home, err := os.UserHomeDir()
	if err != nil {
		return
	}
	dir := filepath.Join(home, ".dirac")
	os.MkdirAll(dir, 0700)
	codexTokens.path = filepath.Join(dir, "codex-auth.json")
}

// Load reads tokens from disk.
func (s *codexTokenStore) Load() (*CodexAuthTokens, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

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

// Save writes tokens to disk with restricted permissions.
func (s *codexTokenStore) Save(tokens *CodexAuthTokens) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.path == "" {
		return fmt.Errorf("token path not set")
	}

	data, err := json.MarshalIndent(tokens, "", "  ")
	if err != nil {
		return err
	}

	return os.WriteFile(s.path, data, 0600)
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

	// Try to refresh
	newTokens, err := codexRefreshToken(tokens.RefreshToken)
	if err != nil {
		return "", fmt.Errorf("token refresh failed: %w", err)
	}

	if err := s.Save(newTokens); err != nil {
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
		listener, err = net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
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
	q.Set("client_id", codexClientID)
	q.Set("redirect_uri", redirectURI)
	q.Set("scope", codexScope)
	q.Set("code_challenge", challenge)
	q.Set("code_challenge_method", "S256")
	q.Set("state", randomState())
	q.Set("codex_cli_simplified_flow", "true")
	authURL.RawQuery = q.Encode()

	// Channel to receive the auth code
	codeChan := make(chan string, 1)
	errChan := make(chan error, 1)

	// Start HTTP server for callback
	srv := &http.Server{}
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

		code := r.URL.Query().Get("code")
		if code == "" {
			errChan <- fmt.Errorf("no authorization code in callback")
			w.WriteHeader(400)
			w.Write([]byte("Error: missing authorization code"))
			return
		}
		codeChan <- code
		w.Write([]byte("Authentication successful! You can close this tab."))
		// Shutdown server after receiving callback
		go srv.Shutdown(context.Background())
	})

	go srv.Serve(listener)

	// Open browser
	if err := openBrowser(authURL.String()); err != nil {
		return nil, fmt.Errorf("failed to open browser: %w", err)
	}

	log.Printf("Waiting for Codex OAuth callback on port %d...", port)

	// Wait for callback or context cancellation
	select {
	case code := <-codeChan:
		return codexExchangeCode(code, verifier, redirectURI)
	case err := <-errChan:
		return nil, err
	case <-ctx.Done():
		srv.Shutdown(context.Background())
		return nil, ctx.Err()
	}
}

// codexExchangeCode exchanges an authorization code for tokens.
func codexExchangeCode(code, verifier, redirectURI string) (*CodexAuthTokens, error) {
	// Step 1: Exchange code for tokens
	form := url.Values{
		"grant_type":    {"authorization_code"},
		"code":          {code},
		"redirect_uri":  {redirectURI},
		"client_id":     {codexClientID},
		"code_verifier": {verifier},
	}

	resp, err := http.PostForm(codexTokenURL, form)
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
		// If token exchange fails, fall back to the access token directly
		log.Printf("Warning: API token exchange failed (%v), using access token directly", err)
		apiToken = tokenResp.AccessToken
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
		"client_id":            {codexClientID},
		"requested_token":      {"openai-api-key"},
		"subject_token":        {idToken},
		"subject_token_type":   {"urn:ietf:params:oauth:token-type:id_token"},
	}

	resp, err := http.PostForm(codexTokenURL, form)
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
	payload, _ := json.Marshal(map[string]interface{}{
		"client_id":     codexClientID,
		"grant_type":    "refresh_token",
		"refresh_token": refreshToken,
		"scope":         "openid profile email offline_access",
	})

	resp, err := http.Post(codexTokenURL, "application/json", strings.NewReader(string(payload)))
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
		log.Printf("Warning: API token exchange on refresh failed (%v), using access token directly", err)
		apiToken = tokenResp.AccessToken
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
		"client_id": codexClientID,
	})

	resp, err := http.Post("https://auth.openai.com/api/accounts/deviceauth/usercode", "application/json", strings.NewReader(string(payload)))
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
	ticker := time.NewTicker(time.Duration(dc.Interval) * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-ticker.C:
			if time.Now().Unix() > dc.ExpiresAt {
				return nil, fmt.Errorf("device code expired")
			}

			payload, _ := json.Marshal(map[string]string{
				"device_auth_id": dc.deviceAuthID,
				"user_code":      dc.UserCode,
			})

			resp, err := http.Post("https://auth.openai.com/api/accounts/deviceauth/token", "application/json", strings.NewReader(string(payload)))
			if err != nil {
				continue // retry on network errors
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

				// Exchange the authorization code for tokens
				verifier := result.CodeVerifier
				if verifier == "" {
					verifier = dc.codeVerifier
				}
				return codexExchangeCode(result.AuthorizationCode, verifier, "")
			}
			// Non-200 means still pending, continue polling
		}
	}
}

// --- Helpers ---

func randomState() string {
	b := make([]byte, 32)
	rand.Read(b)
	return base64.URLEncoding.WithPadding(base64.NoPadding).EncodeToString(b)
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

	return exec.Command(cmd, args...).Start()
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
