import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

export const web_fetch: DiracToolSpec = {
	id: DiracDefaultTool.WEB_FETCH,
	name: "web_fetch",
	description: `Fetch and analyze content from a specified URL using your prompt.
- Takes a URL and analysis prompt as input
- Fetches the URL content and processes based on your prompt
- Use this tool when you need to retrieve and analyze web content
- The URL must be a fully-formed valid URL
- The prompt must be at least 2 characters
- HTTP URLs will be automatically upgraded to HTTPS
- This tool is read-only and does not modify any files
- For searching multiple sources, use web_search instead

Usage: web_fetch <url> --prompt <text>

Positional:
  url                 The URL to fetch content from.

Options:
  --prompt TEXT       (required) The prompt to use for analyzing the webpage content.

Example: web_fetch https://example.com/docs --prompt "Summarize the main points and key takeaways"

Response: OK | url:<url> | tokens:N
	Analyzed content follows header line.
Typical: web_fetch https://example.com/docs --prompt "Summarize key points"`,
	contextRequirements: (ctx) => ctx.diracWebToolsEnabled === true,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for web_fetch.",
			usage: "https://example.com/docs --prompt \"Summarize the main points\"",
		},
	],
	metadata: {
		tags: ["web", "fetch", "url"],
		category: "web",
		concurrency: "parallel-safe",
		safety: ["network"],
		outputSize: "large",
		llmsBrief: "Fetch and read content from URLs",
		compactionSafety: "summarizable",
	},
}
