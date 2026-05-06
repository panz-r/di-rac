import { describe, expect, it } from "vitest"
import { findMostRecentTaskForWorkspace } from "./task-history"

describe("findMostRecentTaskForWorkspace", () => {
	it("returns the newest matching task for the workspace", () => {
		const result = findMostRecentTaskForWorkspace(
			[
				{
					id: "older",
					ts: 100,
					task: "Older task",
					tokensIn: 0,
					tokensOut: 0,
					cwdOnTaskInitialization: "/repo",
				},
				{
					id: "newer",
					ts: 200,
					task: "Newer task",
					tokensIn: 0,
					tokensOut: 0,
					cwdOnTaskInitialization: "/repo",
				},
			],
			"/repo",
		)

		expect(result?.id).toBe("newer")
	})

	it("returns null when there is no match", () => {
		const result = findMostRecentTaskForWorkspace(
			[
				{
					id: "other",
					ts: 200,
					task: "Other task",
					tokensIn: 0,
					tokensOut: 0,
					cwdOnTaskInitialization: "/other",
				},
			],
			"/repo",
		)

		expect(result).toBeNull()
	})
})
