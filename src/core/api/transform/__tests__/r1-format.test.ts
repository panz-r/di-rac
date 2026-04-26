
import { expect } from "chai"
import { addReasoningContent } from "../r1-format"
import { DiracStorageMessage } from "@/shared/messages/content"
import OpenAI from "openai"

describe("r1-format", () => {
	describe("addReasoningContent", () => {
		it("should NOT add reasoning_content if empty", () => {
			const originalMessages: DiracStorageMessage[] = [
				{
					role: "assistant",
					content: [
						{
							type: "text",
							text: "Hello",
						},
					],
				},
			]
			const openAiMessages: OpenAI.Chat.ChatCompletionMessageParam[] = [
				{
					role: "assistant",
					content: "Hello",
				},
			]

			const result = addReasoningContent(openAiMessages, originalMessages)
			expect(result[0]).to.not.have.property("reasoning_content")
		})

		it("should preserve null content for assistant messages", () => {
			const originalMessages: DiracStorageMessage[] = [
				{
					role: "assistant",
					content: [
						{
							type: "tool_use",
							id: "1",
							name: "test",
							input: {},
						},
					],
				},
			]
			const openAiMessages: OpenAI.Chat.ChatCompletionMessageParam[] = [
				{
					role: "assistant",
					content: null as any,
					tool_calls: [
						{
							id: "1",
							type: "function",
							function: { name: "test", arguments: "{}" },
						},
					],
				},
			]

			const result = addReasoningContent(openAiMessages, originalMessages)
			expect(result[0].content).to.equal(null)
			expect(result[0]).to.not.have.property("reasoning_content")
		})

		it("should correctly map thinking content", () => {
			const originalMessages: DiracStorageMessage[] = [
				{
					role: "assistant",
					content: [
						{
							type: "thinking",
							thinking: "I am thinking",
							signature: "test-signature",
						},
						{
							type: "text",
							text: "Hello",
						},
					],
				},
			]
			const openAiMessages: OpenAI.Chat.ChatCompletionMessageParam[] = [
				{
					role: "assistant",
					content: "Hello",
				},
			]

			const result = addReasoningContent(openAiMessages, originalMessages)
			expect(result[0]).to.have.property("reasoning_content", "I am thinking")
			expect(result[0].content).to.equal("Hello")
		})
	})
})
