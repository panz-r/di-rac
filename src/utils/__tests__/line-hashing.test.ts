import { describe, it, expect } from "vitest"
import {
	hashLine,
	generateFullAnchoredContent,
	parseAnchorFromLine,
	formatAnchoredLine,
	contentHash,
} from "../../utils/line-hashing"
import { ANCHOR_DELIMITER } from "../../shared/utils/delimiter"

describe("line-hashing", () => {
	describe("hashLine", () => {
		it("returns 3-char string for any input", () => {
			expect(hashLine("hello").length).toBe(3)
			expect(hashLine("").length).toBe(3)
			expect(hashLine("x".repeat(1000)).length).toBe(3)
		})

		it("same content gives same hash", () => {
			const line = "def process(data):"
			expect(hashLine(line)).toBe(hashLine(line))
		})

		it("different content gives different hash", () => {
			expect(hashLine("abc")).not.toBe(hashLine("xyz"))
		})
	})

	describe("formatAnchoredLine", () => {
		it("produces gutter format with line number", () => {
			const result = formatAnchoredLine("code", "a3", 42)
			expect(result).toBe(`  42 │ a3${ANCHOR_DELIMITER}code`)
		})

		it("pads line numbers to minimum 4 chars", () => {
			const result = formatAnchoredLine("code", "b7", 1)
			expect(result).toBe(`   1 │ b7${ANCHOR_DELIMITER}code`)
		})

		it("without line number returns just hash and content", () => {
			const result = formatAnchoredLine("code", "a3")
			expect(result).toBe(`a3${ANCHOR_DELIMITER}code`)
		})

		it("handles empty content", () => {
			const result = formatAnchoredLine("", "x9", 5)
			expect(result).toBe(`   5 │ x9${ANCHOR_DELIMITER}`)
		})
	})

	describe("generateFullAnchoredContent", () => {
		it("returns array of formatted lines", () => {
			const lines = ["def foo():", "    return 1", ""]
			const result = generateFullAnchoredContent(lines)
			expect(result.length).toBe(3)
			// Each line has gutter + hash + delimiter
			for (const entry of result) {
				expect(entry).toMatch(/^\s+\d+\s+[│|]\s+[a-z0-9_]+\|/)
			}
		})

		it("all hashes in output are unique", () => {
			const lines: string[] = []
			for (let i = 0; i < 50; i++) {
				lines.push(`line ${i}`)
			}
			const result = generateFullAnchoredContent(lines)
			const hashes = result.map((r) => {
				const match = r.match(/[│|]\s+([a-z0-9_]+)\|/)
				return match ? match[1] : ""
			})
			expect(new Set(hashes).size).toBe(hashes.length)
		})
	})

	describe("parseAnchorFromLine", () => {
		it("extracts hash and content from simple format", () => {
			const result = parseAnchorFromLine(`a3${ANCHOR_DELIMITER}def foo()`)
			expect(result).not.toBeNull()
			expect(result!.hash).toBe("a3")
			expect(result!.content).toBe("def foo()")
		})

		it("extracts hash and content from gutter format", () => {
			const result = parseAnchorFromLine(`  42 │ k7${ANCHOR_DELIMITER}def bar()`)
			expect(result).not.toBeNull()
			expect(result!.hash).toBe("k7")
			expect(result!.content).toBe("def bar()")
		})

		it("handles empty content", () => {
			const result = parseAnchorFromLine(`x9${ANCHOR_DELIMITER}`)
			expect(result).not.toBeNull()
			expect(result!.hash).toBe("x9")
			expect(result!.content).toBe("")
		})

		it("handles collision suffix in hash", () => {
			const result = parseAnchorFromLine(`b5_1${ANCHOR_DELIMITER}some code`)
			expect(result).not.toBeNull()
			expect(result!.hash).toBe("b5_1")
			expect(result!.content).toBe("some code")
		})

		it("returns null for invalid format", () => {
			expect(parseAnchorFromLine("")).toBeNull()
			expect(parseAnchorFromLine("no_delimiter_here")).toBeNull()
			expect(parseAnchorFromLine(`AB${ANCHOR_DELIMITER}content`)).toBeNull() // uppercase not valid
		})

		it("parses just a hash (no delimiter)", () => {
			const result = parseAnchorFromLine("a3")
			expect(result).not.toBeNull()
			expect(result!.hash).toBe("a3")
			expect(result!.content).toBe("")
		})
	})

	describe("contentHash", () => {
		it("returns 8-char hex string", () => {
			const hash = contentHash("test content")
			expect(hash.length).toBe(8)
			expect(hash).toMatch(/^[0-9a-f]{8}$/)
		})

		it("is deterministic", () => {
			const h1 = contentHash("hello")
			const h2 = contentHash("hello")
			expect(h1).toBe(h2)
		})

		it("different content gives different hash", () => {
			const h1 = contentHash("hello")
			const h2 = contentHash("world")
			expect(h1).not.toBe(h2)
		})

		it("empty string returns valid hash", () => {
			const hash = contentHash("")
			expect(hash.length).toBe(8)
		})
	})
})
