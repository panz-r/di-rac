import { describe, it, expect } from "vitest"
import { xxHash32, encodeShortHash, computeLineHash, getAlphabet } from "../hash-utils"

describe("hash-utils", () => {
	describe("getAlphabet", () => {
		it("returns the 31-char alphabet with digits and consonants", () => {
			const alphabet = getAlphabet()
			expect(alphabet).toBe("0123456789bcdfghjklmnpqrstvwxyz")
			expect(alphabet.length).toBe(31)
		})

		it("contains no vowels", () => {
			const alphabet = getAlphabet()
			const vowels = ["a", "e", "i", "o", "u"]
			for (const v of vowels) {
				expect(alphabet).not.toContain(v)
			}
		})
	})

	describe("xxHash32", () => {
		it("is deterministic: same input gives same output", () => {
			const input = "hello world"
			const a = xxHash32(input)
			const b = xxHash32(input)
			expect(a).toBe(b)
		})

		it("different inputs give different outputs", () => {
			const h1 = xxHash32("hello")
			const h2 = xxHash32("world")
			expect(h1).not.toBe(h2)
		})

		it("empty string returns valid number", () => {
			const h = xxHash32("")
			expect(typeof h).toBe("number")
			expect(h).toBeGreaterThanOrEqual(0)
		})

		it("returns unsigned 32-bit number", () => {
			const h = xxHash32("test")
			expect(h).toBeGreaterThanOrEqual(0)
			expect(h).toBeLessThan(2 ** 32)
		})
	})

	describe("encodeShortHash", () => {
		it("returns default 3-char string", () => {
			const encoded = encodeShortHash(12345)
			expect(encoded.length).toBe(3)
		})

		it("only contains alphabet characters", () => {
			const alphabet = new Set(getAlphabet())
			for (let i = 0; i < 1000; i++) {
				const encoded = encodeShortHash(i * 7 + 13)
				for (const ch of encoded) {
					expect(alphabet.has(ch)).toBe(true)
				}
			}
		})

		it("different inputs produce different encodings", () => {
			const e1 = encodeShortHash(0)
			const e2 = encodeShortHash(1)
			expect(e1).not.toBe(e2)
		})

		it("returns different outputs for different hash values", () => {
			const seen = new Set<string>()
			for (let i = 0; i < 1000; i++) {
				const encoded = encodeShortHash(i * 12345)
				seen.add(encoded)
			}
			expect(seen.size).toBe(1000)
		})
	})

	describe("computeLineHash", () => {
		it("returns 3-char string", () => {
			const hash = computeLineHash("def foo():")
			expect(hash.length).toBe(3)
		})

		it("same content produces same hash", () => {
			const line = "    return x + 1"
			expect(computeLineHash(line)).toBe(computeLineHash(line))
		})

		it("different content produces different hash", () => {
			const h1 = computeLineHash("def foo():")
			const h2 = computeLineHash("def bar():")
			expect(h1).not.toBe(h2)
		})

		it("whitespace differences produce different hashes", () => {
			const h1 = computeLineHash("def foo():")
			const h2 = computeLineHash("  def foo():")
			expect(h1).not.toBe(h2)
		})

		it("empty line produces valid hash", () => {
			const hash = computeLineHash("")
			expect(hash.length).toBe(3)
		})
	})
})
