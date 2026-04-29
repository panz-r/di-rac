import { APIError } from "openai"
import { expect } from "chai"
import { checkContextWindowExceededError } from "../context-error-handling"

describe("checkContextWindowExceededError", () => {
	it("detects OpenRouter context errors using structured status", () => {
		const error = Object.assign(
			new Error("This endpoint's maximum context length is 204800 tokens. However, you requested about 244027 tokens."),
			{
				status: 400,
			},
		)

		expect(checkContextWindowExceededError(error)).to.equal(true)
	})

	it("detects OpenRouter JSON-encoded status + context length errors", () => {
		const error = new Error(
			'OpenRouter Mid-Stream Error: {"status":400,"message":"This endpoint\'s maximum context length is 200000 tokens"}',
		)

		expect(checkContextWindowExceededError(error)).to.equal(true)
	})

	it("does not classify unrelated 400 errors as context window failures", () => {
		const error = new Error("OpenRouter API Error 400: Invalid API key")

		expect(checkContextWindowExceededError(error)).to.equal(false)
	})

	// MiniMax error detection
	describe("MiniMax", () => {
		it("detects MiniMax error code 1039 (Token limitation)", () => {
			const error = {
				error: {
					error: {
						type: "invalid_request_error",
						code: "1039",
						message: "Token limitation",
					},
				},
			}

			expect(checkContextWindowExceededError(error)).to.equal(true)
		})

		it("detects MiniMax error code 2056 (Token Plan exceeded)", () => {
			const error = {
				error: {
					error: {
						type: "invalid_request_error",
						code: "2056",
						message: "Token Plan exceeded",
					},
				},
			}

			expect(checkContextWindowExceededError(error)).to.equal(true)
		})

		it("detects MiniMax 400 error with token limit message", () => {
			const error = Object.assign(new Error("Token limit exceeded for this request"), {
				status: 400,
			})

			expect(checkContextWindowExceededError(error)).to.equal(true)
		})

		it("detects MiniMax 400 error with context length message", () => {
			const error = {
				status: 400,
				message: "Context length exceeded the maximum allowed",
				error: { message: "Context length exceeded the maximum allowed" },
			}

			expect(checkContextWindowExceededError(error)).to.equal(true)
		})

		it("does not classify unrelated MiniMax errors as context failures", () => {
			const error = {
				status: 400,
				message: "Invalid request format",
			}

			expect(checkContextWindowExceededError(error)).to.equal(false)
		})
	})

	// Z.ai error detection
	describe("Z.ai", () => {
		it("detects Z.ai error code 1261 (Prompt exceeds max length)", () => {
			const error = new APIError(400, {} as any, "Prompt exceeds max length", {})
			Object.defineProperty(error, "code", { value: "1261" })

			expect(checkContextWindowExceededError(error)).to.equal(true)
		})

		it("detects Z.ai raw code 1261 without APIError instance", () => {
			const error = {
				code: "1261",
				message: "Prompt exceeds max length",
			}

			expect(checkContextWindowExceededError(error)).to.equal(true)
		})

		it("detects Z.ai 400 error with prompt exceeds message", () => {
			const error = Object.assign(new Error("Prompt exceeds max length limit"), {
				status: 400,
			})

			expect(checkContextWindowExceededError(error)).to.equal(true)
		})

		it("detects Z.ai 400 error with context window exceed message", () => {
			const error = {
				status: 400,
				message: "Context window exceeded for this model",
				error: { message: "Context window exceeded for this model" },
			}

			expect(checkContextWindowExceededError(error)).to.equal(true)
		})

		it("does not classify unrelated Z.ai errors as context failures", () => {
			const error = Object.assign(new Error("Invalid model specified"), {
				status: 400,
			})

			expect(checkContextWindowExceededError(error)).to.equal(false)
		})
	})
})
