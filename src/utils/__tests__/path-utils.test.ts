import { strict as assert } from "node:assert"
import * as path from "node:path"
import { normalizePath, PathEscapeError } from "../path-utils"

describe("path-utils", () => {
	const workspaceRoot = process.platform === "win32" ? "C:\\project" : "/home/user/project"

	it("normalizes simple relative paths", () => {
		const raw = "src/main.ts"
		const result = normalizePath(raw, workspaceRoot)
		assert.equal(result, "src/main.ts")
	})

	it("normalizes absolute paths within workspace", () => {
		const raw = path.join(workspaceRoot, "src/main.ts")
		const result = normalizePath(raw, workspaceRoot)
		assert.equal(result, "src/main.ts")
	})

	it("normalizes paths with ./ and removes them", () => {
		const raw = "./src/utils/../main.ts"
		const result = normalizePath(raw, workspaceRoot)
		assert.equal(result, "src/main.ts")
	})

	it("throws PathEscapeError for paths escaping root via ..", () => {
		const raw = "../outside.ts"
		assert.throws(() => normalizePath(raw, workspaceRoot), PathEscapeError)
	})

	it("throws PathEscapeError for absolute paths outside root", () => {
		const outside = process.platform === "win32" ? "C:\\windows\\system32" : "/etc/passwd"
		assert.throws(() => normalizePath(outside, workspaceRoot), PathEscapeError)
	})

	it("handles trailing slashes by removing them", () => {
		const raw = "src/utils/"
		const result = normalizePath(raw, workspaceRoot)
		assert.equal(result, "src/utils")
	})

	it("handles the root itself", () => {
		const result = normalizePath(".", workspaceRoot)
		assert.equal(result, ".")
	})
})
