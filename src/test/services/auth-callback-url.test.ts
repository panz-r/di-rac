import { describe, it } from "mocha"
import "should"

/**
 * Regression tests for OAuth callback URL encoding.
 *
 * These tests verify that callback URLs with query params are properly
 * round-tripped through URL encoding via searchParams, preventing
 * truncation of nested query parameters.
 */
describe("Auth Callback URL", () => {
	describe("callback URL encoding", () => {
		it("should preserve callback_url with query params when URL-encoded via searchParams", () => {
			// Simulates a callback URL that contains its own query params.
			// If callers string-interpolate instead of using searchParams.set(),
			// everything after the first & gets parsed as a top-level param and
			// callback_url is truncated.
			const webCallback = "https://codespace-abc.github.dev/callback?tkn=secret123&extra=val"

			const authUrl = new URL("https://openrouter.ai/auth")
			authUrl.searchParams.set("callback_url", webCallback)

			// The callback_url value must round-trip intact
			const parsed = new URL(authUrl.toString())
			parsed.searchParams.get("callback_url")!.should.equal(webCallback)

			// The raw URL must NOT contain an unencoded & from the callback
			const raw = authUrl.toString()
			raw.should.not.containEql("&extra=")
			raw.should.not.containEql("&tkn=")
			raw.should.containEql(encodeURIComponent("&extra=val"))
		})

		it("should encode vscode:// callback URLs correctly", () => {
			const desktopCallback = "vscode://dirac-run.dirac/openrouter"

			const authUrl = new URL("https://openrouter.ai/auth")
			authUrl.searchParams.set("callback_url", desktopCallback)

			const parsed = new URL(authUrl.toString())
			parsed.searchParams.get("callback_url")!.should.equal(desktopCallback)
		})
	})
})
