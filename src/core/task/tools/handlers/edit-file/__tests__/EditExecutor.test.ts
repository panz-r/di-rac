import { describe, it, expect } from "vitest"
import { EditExecutor } from "../EditExecutor"
import { FileAnchorIndex } from "@shared/utils/file-anchor-index"
import { ANCHOR_DELIMITER } from "@shared/utils/delimiter"
import { ToolUse } from "@core/assistant-message"

function makeEditBlock(
	anchor: string,
	text: string,
	endAnchor?: string,
	editType: "replace" | "insert_after" | "insert_before" = "replace",
): ToolUse {
	return {
		type: "tool_use",
		name: "edit_file",
		params: {
			edits: [
				{
					anchor,
					end_anchor: endAnchor || anchor,
					edit_type: editType,
					text,
				},
			],
		},
	} as ToolUse
}

describe("EditExecutor", () => {
	const executor = new EditExecutor()

	describe("resolveEdits — language-aware whitespace", () => {
		it("Python files use exact match (no normalization)", () => {
			const content = ["def foo():", "    return 1"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			// Provide content with trailing space — should fail for Python
			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}def foo():`,
				`${h1}${ANCHOR_DELIMITER}    return 1`,
			)
			const { resolvedEdits, failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/script.py",
			)
			expect(resolvedEdits.length).toBe(1)
			expect(failedEdits.length).toBe(0)
		})

		it("Python files reject indentation differences", () => {
			const content = ["def foo():", "    return 1"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			// Provide line 1 with different indentation (2 spaces instead of 4)
			// Pass both as a range: anchor points to line 0 (match), end_anchor to line 1 (mismatch)
			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}def foo():`,
				"new content",
				`${h1}${ANCHOR_DELIMITER}  return 1`,
			)
			const { resolvedEdits, failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/script.py",
			)
			expect(resolvedEdits.length).toBe(0)
			expect(failedEdits.length).toBe(1)
			expect(failedEdits[0].error).toContain("stale")
		})

		it("Python files reject content mismatch in end_anchor", () => {
			const content = ["def foo():", "    return 1"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			// Provide end_anchor hash correct but content differs (wrong return value)
			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}def foo():`,
				"new content",
				`${h1}${ANCHOR_DELIMITER}    return 99`,
			)
			const { failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/script.py",
			)
			expect(failedEdits.length).toBe(1)
			expect(failedEdits[0].error).toContain("stale")
		})

		it("Haskell files use exact match", () => {
			const content = ["main :: IO ()", "main = putStrLn \"hello\""]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}main :: IO ()`,
				`${h1}${ANCHOR_DELIMITER}main = putStrLn "hello"`,
			)
			const { resolvedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/Main.hs",
			)
			expect(resolvedEdits.length).toBe(1)
		})

		it("YAML files use exact match", () => {
			const content = ["key: value", "  nested: true"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			// Provide content that matches exactly
			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}key: value`,
				`${h1}${ANCHOR_DELIMITER}  nested: true`,
			)
			const { resolvedEdits, failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/config.yml",
			)
			expect(resolvedEdits.length).toBe(1)
			expect(failedEdits.length).toBe(0)
		})

		it("YAML files reject whitespace differences in indentation", () => {
			const content = ["key: value", "  nested: true"]
			const index = new FileAnchorIndex(content)
			const h1 = index.getHash(1)

			// Provide different indentation (single space vs two)
			const block = makeEditBlock(
				`${h1}${ANCHOR_DELIMITER} nested: true`,
				`${h1}${ANCHOR_DELIMITER} nested: true`,
			)
			const { failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/config.yml",
			)
			expect(failedEdits.length).toBe(1)
		})

		it("TypeScript files normalize tabs to spaces", () => {
			const content = ["const x = 1;", "    return x;"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			// Provided uses tab instead of spaces — normalized match for TS
			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}const x = 1;`,
				`${h1}${ANCHOR_DELIMITER}\treturn x;`,
			)
			const { resolvedEdits, failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.ts",
			)
			expect(resolvedEdits.length).toBe(1)
			expect(failedEdits.length).toBe(0)
		})

		it("JavaScript files normalize tabs to spaces", () => {
			const content = ["function foo() {", "    return 1;", "}"]
			const index = new FileAnchorIndex(content)
			const h1 = index.getHash(1)

			// Provided has tab instead of spaces
			const block = makeEditBlock(
				`${h1}${ANCHOR_DELIMITER}\treturn 1;`,
				`${h1}${ANCHOR_DELIMITER}\treturn 1;`,
			)
			const { resolvedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/script.js",
			)
			expect(resolvedEdits.length).toBe(1)
		})

		it("Java files normalize whitespace", () => {
			const content = ["public class Test {", "    int x = 1;", "}"]
			const index = new FileAnchorIndex(content)
			const h1 = index.getHash(1)

			const block = makeEditBlock(
				`${h1}${ANCHOR_DELIMITER}    int x = 1; `,
				`${h1}${ANCHOR_DELIMITER}    int x = 1; `,
			)
			const { resolvedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/Test.java",
			)
			expect(resolvedEdits.length).toBe(1)
		})

		it("Go files normalize whitespace", () => {
			const content = ["func main() {", "\tfmt.Println(\"hi\")", "}"]
			const index = new FileAnchorIndex(content)
			const h1 = index.getHash(1)

			const block = makeEditBlock(
				`${h1}${ANCHOR_DELIMITER}    fmt.Println("hi")`,
				`${h1}${ANCHOR_DELIMITER}    fmt.Println("hi")`,
			)
			const { resolvedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/main.go",
			)
			expect(resolvedEdits.length).toBe(1)
		})

		it("Makefile uses exact match", () => {
			const content = ["all: build", "", "build:", "\tgcc -o main main.c"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h3 = index.getHash(3)

			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}all: build`,
				`${h3}${ANCHOR_DELIMITER}\tgcc -o main main.c`,
			)
			const { resolvedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/Makefile",
			)
			expect(resolvedEdits.length).toBe(1)
		})

		it("unknown file extensions default to normalization", () => {
			const content = ["some random text  "]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)

			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}some random text`,
				`${h0}${ANCHOR_DELIMITER}some random text`,
			)
			const { resolvedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/unknown.xyz",
			)
			// Trailing space was normalized away
			expect(resolvedEdits.length).toBe(1)
		})
	})

	describe("resolveEdits — echo stripping", () => {
		it("strips accidental anchor prefix from provided content", () => {
			const content = ["def foo():", "    pass"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			// LLM echoes back anchor in the content
			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}def foo():`,
				`${h1}${ANCHOR_DELIMITER}${h1}${ANCHOR_DELIMITER}    pass`,
			)
			const { resolvedEdits, failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.ts",
			)
			expect(resolvedEdits.length).toBe(1)
			expect(failedEdits.length).toBe(0)
		})

		it("does not strip anchor that appears mid-content", () => {
			const content = ["x = 1", `print("abc|def")`]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			// The | in a string is not at the start so it's fine
			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}x = 1`,
				`${h1}${ANCHOR_DELIMITER}print("abc|def")`,
			)
			const { resolvedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.py",
			)
			expect(resolvedEdits.length).toBe(1)
		})
	})

	describe("resolveEdits — error feedback", () => {
		it("returns suggestions for unknown anchor (close Levenshtein match)", () => {
			const content = ["line one", "line two", "line three"]
			const index = new FileAnchorIndex(content)
			const hashes = index.getAllHashes()

			// Try a hash that is close to one of the actual hashes
			const closeToReal = hashes[0] // use a real one, but slightly off
			const fakeHash = closeToReal.length === 3
				? closeToReal.substring(0, 2) + "x"
				: "xyz"

			const block = makeEditBlock(
				`${fakeHash}${ANCHOR_DELIMITER}some content`,
				`${fakeHash}${ANCHOR_DELIMITER}some content`,
			)
			const { failedEdits } = executor.resolveEdits(
				[block],
				content,
				hashes,
				index,
				"/test/file.ts",
			)
			expect(failedEdits.length).toBe(1)
			expect(failedEdits[0].error).toContain("not found")
		})

		it("returns suggestion with line number for unknown anchor", () => {
			const content = Array(20).fill("").map((_, i) => `line ${i}`)
			const index = new FileAnchorIndex(content)

			const block = makeEditBlock(
				`zzz${ANCHOR_DELIMITER}some content`,
				`zzz${ANCHOR_DELIMITER}some content`,
			)
			const { failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.ts",
			)
			expect(failedEdits.length).toBe(1)
			// Should contain a suggestion message
			const err = failedEdits[0].error
			expect(err.toLowerCase()).toMatch(/not found|did you mean/)
		})

		it("stale anchor returns error with current anchor and content", () => {
			const content = ["def foo():", "    return 1"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			// The hash for line 0 is correct, but the content doesn't match
			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}def bar():`,
				`${h1}${ANCHOR_DELIMITER}    return 1`,
			)
			const { failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.ts",
			)
			expect(failedEdits.length).toBe(1)
			expect(failedEdits[0].error).toContain("stale")
			expect(failedEdits[0].error).toContain(h0)
		})

		it("resolves collision suffix anchor correctly", () => {
			const content = ["dup", "dup", "dup"]
			const index = new FileAnchorIndex(content)
			const h1 = index.getHash(1)
			const h2 = index.getHash(2)

			// Resolve the collision suffix — use both anchors as a range
			const block = makeEditBlock(
				`${h1}${ANCHOR_DELIMITER}dup`,
				"new content",
				`${h2}${ANCHOR_DELIMITER}dup`,
			)
			const { resolvedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.ts",
			)
			expect(resolvedEdits.length).toBe(1)
			expect(resolvedEdits[0].lineIdx).toBe(1)
			expect(resolvedEdits[0].endIdx).toBe(2)
		})
	})

	describe("resolveEdits — edge cases", () => {
		it("rejects edits with newlines in anchor content", () => {
			const content = ["line one", "line two"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)

			const block = makeEditBlock(
				`${h0}${ANCHOR_DELIMITER}line\none`,
				`${h0}${ANCHOR_DELIMITER}line\none`,
			)
			const { failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.ts",
			)
			expect(failedEdits.length).toBe(1)
			expect(failedEdits[0].error).toContain("newline")
		})

		it("empty anchor string returns error", () => {
			const content = ["test"]
			const index = new FileAnchorIndex(content)

			const block = makeEditBlock("", "some text")
			const { failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.ts",
			)
			expect(failedEdits.length).toBe(1)
			expect(failedEdits[0].error).toContain("missing")
		})

		it("range error when end_anchor before anchor", () => {
			const content = ["line A", "line B", "line C"]
			const index = new FileAnchorIndex(content)
			const h2 = index.getHash(2)
			const h0 = index.getHash(0)

			// end_anchor (h0 pointing to line 0) before anchor (h2 pointing to line 2)
			const block = makeEditBlock(
				`${h2}${ANCHOR_DELIMITER}line C`,
				"new content",
				`${h0}${ANCHOR_DELIMITER}line A`,
			)
			const { failedEdits } = executor.resolveEdits(
				[block],
				content,
				index.getAllHashes(),
				index,
				"/test/file.ts",
			)
			expect(failedEdits.length).toBe(1)
			expect(failedEdits[0].error).toContain("Range error")
		})
	})

	describe("applyEdits", () => {
		it("applies replace edit and updates FileAnchorIndex", () => {
			const content = ["def foo():", "    pass", ""]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)
			const h1 = index.getHash(1)

			const replacedContent = "def bar():\n    return 42"
			const edits = [{
				lineIdx: 0,
				endIdx: 1,
				edit: {
					anchor: `${h0}${ANCHOR_DELIMITER}def foo():`,
					end_anchor: `${h1}${ANCHOR_DELIMITER}    pass`,
					edit_type: "replace" as const,
					text: replacedContent,
				},
			}]

			const { finalLines, addedCount, removedCount } = executor.applyEdits(content, edits, index)
			expect(finalLines).toEqual(["def bar():", "    return 42", ""])
			expect(addedCount).toBe(2)
			expect(removedCount).toBe(2)

			// Index should be updated
			const newH0 = index.getHash(0)
			expect(index.getLineIdx(newH0)).toBe(0)
			expect(index.getLine(0)).toBe("def bar():")
		})

		it("applies insert_after edit", () => {
			const content = ["line one", "line two"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)

			const edits = [{
				lineIdx: 0,
				endIdx: 0,
				edit: {
					anchor: `${h0}${ANCHOR_DELIMITER}line one`,
					edit_type: "insert_after" as const,
					text: "inserted line",
				},
			}]

			const { finalLines } = executor.applyEdits(content, edits, index)
			expect(finalLines).toEqual(["line one", "inserted line", "line two"])
		})

		it("applies insert_before edit", () => {
			const content = ["line one", "line two"]
			const index = new FileAnchorIndex(content)
			const h0 = index.getHash(0)

			const edits = [{
				lineIdx: 0,
				endIdx: 0,
				edit: {
					anchor: `${h0}${ANCHOR_DELIMITER}line one`,
					edit_type: "insert_before" as const,
					text: "inserted line",
				},
			}]

			const { finalLines } = executor.applyEdits(content, edits, index)
			expect(finalLines).toEqual(["inserted line", "line one", "line two"])
		})

		it("two edits in same batch apply bottom-up correctly", () => {
			const content = ["A", "B", "C", "D", "E"]
			const index = new FileAnchorIndex(content)
			const h1 = index.getHash(1)
			const h3 = index.getHash(3)

			// Edit B (line 1) and D (line 3) — bottom-up means D first, then B
			const edits = [
				{
					lineIdx: 3,
					endIdx: 3,
					edit: {
						anchor: `${h3}${ANCHOR_DELIMITER}D`,
						edit_type: "replace" as const,
						text: "D-new",
					},
				},
				{
					lineIdx: 1,
					endIdx: 1,
					edit: {
						anchor: `${h1}${ANCHOR_DELIMITER}B`,
						edit_type: "replace" as const,
						text: "B-new",
					},
				},
			]

			const { finalLines } = executor.applyEdits(content, edits, index)
			expect(finalLines).toEqual(["A", "B-new", "C", "D-new", "E"])
		})
	})
})
