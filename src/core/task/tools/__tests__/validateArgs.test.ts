/**
 * Unit tests for validateArgs and safeStringArray.
 *
 * Verifies that malformed inputs produce INVALID_ARGUMENT errors and
 * valid inputs pass through unchanged.
 */
import { describe, it } from "mocha";
import "should";
import { z } from "zod";

import { validateArgs, safeStringArray } from "../validateArgs";

describe("validateArgs", () => {
	const SimpleSchema = z.object({
		name: z.string().min(1),
		count: z.number().int().positive(),
	}).strict();

	it("should return success for valid input", () => {
		const result = validateArgs(SimpleSchema, { name: "test", count: 42 }, "test_tool");
		result.success.should.be.true();
		if (result.success) {
			result.data.name.should.equal("test");
			result.data.count.should.equal(42);
		}
	});

	it("should return failure for missing required field", () => {
		const result = validateArgs(SimpleSchema, { name: "test" }, "test_tool");
		result.success.should.be.false();
		if (!result.success) {
			result.error.code.should.equal("arg.invalidArgument");
			result.error.severity.should.equal("recoverable");
			result.error.message.should.containEql("count");
			result.error.details!.should.have.property("received");
			result.error.details!.should.have.property("issues");
		}
	});

	it("should return failure for wrong type", () => {
		const result = validateArgs(SimpleSchema, { name: "test", count: "not-a-number" }, "test_tool");
		result.success.should.be.false();
		if (!result.success) {
			result.error.code.should.equal("arg.invalidArgument");
		}
	});

	it("should return failure for unknown keys with strict mode", () => {
		const result = validateArgs(SimpleSchema, { name: "test", count: 42, extra: "bad" }, "test_tool");
		result.success.should.be.false();
		if (!result.success) {
			result.error.code.should.equal("arg.invalidArgument");
		}
	});

	it("should return failure for null input", () => {
		const result = validateArgs(SimpleSchema, null, "test_tool");
		result.success.should.be.false();
		if (!result.success) {
			result.error.code.should.equal("arg.invalidArgument");
		}
	});

	it("should return failure for undefined input", () => {
		const result = validateArgs(SimpleSchema, undefined, "test_tool");
		result.success.should.be.false();
		if (!result.success) {
			result.error.code.should.equal("arg.invalidArgument");
		}
	});

	it("should include tool name in error message", () => {
		const result = validateArgs(SimpleSchema, {}, "fancy_tool");
		result.success.should.be.false();
		if (!result.success) {
			result.error.message.should.containEql("fancy_tool");
		}
	});
});

describe("validateArgs with string arrays", () => {
	const ArraySchema = z.object({
		paths: z.array(z.string()).min(1),
	}).strict();

	it("should accept valid string arrays", () => {
		const result = validateArgs(ArraySchema, { paths: ["a.txt", "b.txt"] }, "test");
		result.success.should.be.true();
	});

	it("should reject array with numeric element in string position", () => {
		// The number 65 (ASCII 'A') in a string array should fail validation
		const result = validateArgs(ArraySchema, { paths: [42, "file.txt"] }, "test");
		result.success.should.be.false();
		if (!result.success) {
			result.error.code.should.equal("arg.invalidArgument");
		}
	});

	it("should reject empty array", () => {
		const result = validateArgs(ArraySchema, { paths: [] }, "test");
		result.success.should.be.false();
	});
});

describe("validateArgs with JSON-string coercion", () => {
	const ArraySchema = z.object({
		paths: z.preprocess((val) => {
			if (typeof val === "string") {
				try {
					const parsed = JSON.parse(val);
					if (Array.isArray(parsed)) return parsed;
				} catch { /* ignore */ }
			}
			return val;
		}, z.array(z.string()).min(1)),
	}).strict();

	it("should coerce JSON-stringified arrays", () => {
		const result = validateArgs(ArraySchema, { paths: '["a.txt", "b.txt"]' }, "test");
		result.success.should.be.true();
		if (result.success) {
			(result.data.paths as string[]).should.deepEqual(["a.txt", "b.txt"]);
		}
	});

	it("should reject garbage JSON string", () => {
		const result = validateArgs(ArraySchema, { paths: "not-json" }, "test");
		result.success.should.be.false();
	});
});

describe("safeStringArray", () => {
	it("should return array for valid string arrays", () => {
		const result = safeStringArray(["a", "b", "c"]);
		result!.should.deepEqual(["a", "b", "c"]);
	});

	it("should return null for arrays with numeric elements", () => {
		// The ASCII-character-code artefact
		const result = safeStringArray([65, "file.txt"]);
		(result === null).should.be.true();
	});

	it("should return null for non-arrays", () => {
		(safeStringArray("not-an-array") === null).should.be.true();
		(safeStringArray(42) === null).should.be.true();
		(safeStringArray(null) === null).should.be.true();
		(safeStringArray(undefined) === null).should.be.true();
		(safeStringArray({}) === null).should.be.true();
	});

	it("should return empty array for empty input array", () => {
		const result = safeStringArray([]);
		result!.should.deepEqual([]);
	});
});
