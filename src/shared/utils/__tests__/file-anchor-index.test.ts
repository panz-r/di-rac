import { describe, it, expect } from "vitest"
import { FileAnchorIndex } from "../file-anchor-index"
import { ANCHOR_DELIMITER } from "../delimiter"

describe("FileAnchorIndex", () => {
	const sampleContent = [
		"def foo():",
		"    return 1",
		"",
		"def bar():",
		"    return 2",
	]

	describe("construction", () => {
		it("builds correct line-to-hash mapping", () => {
			const index = new FileAnchorIndex(sampleContent)
			expect(index.lineCount).toBe(5)
			for (let i = 0; i < 5; i++) {
				const hash = index.getHash(i)
				expect(hash.length).toBeGreaterThanOrEqual(3)
				expect(hash.length).toBeLessThanOrEqual(5) // 3 + potential _N suffix
			}
		})

		it("same line content gets same hash", () => {
			const content = ["line one", "line two", "line one"] // duplicate
			const index = new FileAnchorIndex(content)
			// First "line one" gets base hash, second gets _1 suffix
			const h0 = index.getHash(0)
			const h2 = index.getHash(2)
			expect(h0).not.toBe(h2) // collision resolved
			expect(h2).toMatch(/^[a-z0-9_]+_\d+$/)
		})

		it("all hashes are unique within a file", () => {
			const content: string[] = []
			for (let i = 0; i < 100; i++) {
				content.push(`line ${i}`)
			}
			const index = new FileAnchorIndex(content)
			const hashes = index.getAllHashes()
			const uniqueHashes = new Set(hashes)
			expect(uniqueHashes.size).toBe(hashes.length)
		})
	})

	describe("collision resolution", () => {
		it("3 lines with same raw hash get a3, a3_0, a3_1 suffixes", () => {
			const content = ["same line", "same line", "same line"]
			const index = new FileAnchorIndex(content)

			const h0 = index.getHash(0)
			const h1 = index.getHash(1)
			const h2 = index.getHash(2)

			expect(h0).not.toBe(h1)
			expect(h0).not.toBe(h2)
			expect(h1).not.toBe(h2)

			expect(h1).toMatch(/^[a-z0-9_]+_\d+$/)
			expect(h2).toMatch(/^[a-z0-9_]+_\d+$/)

			expect(index.getLineIdx(h0)).toBe(0)
			expect(index.getLineIdx(h1)).toBe(1)
			expect(index.getLineIdx(h2)).toBe(2)
		})

		it("resolve a3_1 collision suffix returns correct line", () => {
			const content = ["dup", "dup", "dup"]
			const index = new FileAnchorIndex(content)

			const hash1 = index.getHash(1)
			const hash2 = index.getHash(2)

			expect(index.getLineIdx(hash1)).toBe(1)
			expect(index.getLineIdx(hash2)).toBe(2)
			expect(index.getLine(index.getLineIdx(hash2)!)).toBe("dup")
		})

		it("edit that creates new collision gets appropriate suffix", () => {
			const content = ["unique A", "unique B", "unique C"]
			const index = new FileAnchorIndex(content)

			index.updateLine(1, "unique A")

			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			expect(h0).not.toBe(h1)
			expect(h1).toMatch(/^[a-z0-9_]+_\d+$/)
			expect(index.getLineIdx(h0)).toBe(0)
			expect(index.getLineIdx(h1)).toBe(1)
		})

		it("multiple collisions across large set of identical lines", () => {
			const content = Array(10).fill("identical content line")
			const index = new FileAnchorIndex(content)

			const hashes = index.getAllHashes()
			expect(new Set(hashes).size).toBe(10)

			for (let i = 0; i < 10; i++) {
				expect(index.getLineIdx(hashes[i])).toBe(i)
			}
		})
	})


	describe("getHash and getLineIdx", () => {
		it("roundtrips: getLineIdx(getHash(i)) === i", () => {
			const index = new FileAnchorIndex(sampleContent)
			for (let i = 0; i < sampleContent.length; i++) {
				const hash = index.getHash(i)
				expect(index.getLineIdx(hash)).toBe(i)
			}
		})

		it("getLineIdx returns undefined for unknown hash", () => {
			const index = new FileAnchorIndex(sampleContent)
			expect(index.getLineIdx("zzz")).toBeUndefined()
		})
	})

	describe("getLine", () => {
		it("returns original content", () => {
			const index = new FileAnchorIndex(sampleContent)
			expect(index.getLine(0)).toBe("def foo():")
			expect(index.getLine(2)).toBe("")
			expect(index.getLine(4)).toBe("    return 2")
		})
	})

	describe("getGutterRepresentation", () => {
		it("formats lines with gutter, anchor, and delimiter", () => {
			const index = new FileAnchorIndex(sampleContent)
			const gutter = index.getGutterRepresentation()
			expect(gutter.length).toBe(5)

			for (const line of gutter) {
				// Format: "   42 │ a3|code..."
				expect(line).toMatch(/^\s+\d+\s+[│|]\s+[a-z0-9_]+\|/)
			}
		})

		it("first line has line number 1", () => {
			const index = new FileAnchorIndex(["test"])
			const gutter = index.getGutterRepresentation()
			expect(gutter[0]).toContain("1 │ ")
		})
	})

	describe("updateLine", () => {
		it("updates hash when line content changes", () => {
			const index = new FileAnchorIndex(sampleContent)
			const oldHash = index.getHash(0)
			index.updateLine(0, "def baz():")
			const newHash = index.getHash(0)
			expect(newHash).not.toBe(oldHash)
		})

		it("old hash is no longer in the index", () => {
			const index = new FileAnchorIndex(sampleContent)
			const oldHash = index.getHash(0)
			index.updateLine(0, "new content")
			expect(index.getLineIdx(oldHash)).toBeUndefined()
		})

		it("new hash maps to the correct line", () => {
			const index = new FileAnchorIndex(sampleContent)
			index.updateLine(0, "new content")
			const newHash = index.getHash(0)
			expect(index.getLineIdx(newHash)).toBe(0)
		})
	})

	describe("getAllHashes", () => {
		it("returns array of all hashes in order", () => {
			const index = new FileAnchorIndex(sampleContent)
			const hashes = index.getAllHashes()
			expect(hashes.length).toBe(5)
			for (let i = 0; i < 5; i++) {
				expect(hashes[i]).toBe(index.getHash(i))
			}
		})
	})

	describe("edge cases", () => {
		it("empty content array", () => {
			const index = new FileAnchorIndex([])
			expect(index.lineCount).toBe(0)
			expect(index.getAllHashes()).toEqual([])
			expect(index.getGutterRepresentation()).toEqual([])
		})

		it("single empty line", () => {
			const index = new FileAnchorIndex([""])
			expect(index.lineCount).toBe(1)
			expect(index.getHash(0).length).toBeGreaterThanOrEqual(3)
			expect(index.getLineIdx(index.getHash(0))).toBe(0)
		})

		it("very long line", () => {
			const longLine = "x".repeat(10000)
			const index = new FileAnchorIndex([longLine])
			expect(index.getHash(0).length).toBeGreaterThanOrEqual(3)
		})

		it("lines with delimiter-like characters", () => {
			const content = ["a|b|c", "|||", "test|"]
			const index = new FileAnchorIndex(content)
			expect(index.lineCount).toBe(3)
			// Hashes should still work fine
			for (let i = 0; i < 3; i++) {
				expect(index.getLineIdx(index.getHash(i))).toBe(i)
			}
		})
	})
})
