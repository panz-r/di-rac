import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

export const web_search: DiracToolSpec = {
	id: DiracDefaultTool.WEB_SEARCH,
	name: "web_search",
	description: `Search the web for information. Returns titles, URLs, and snippets.
- Takes a search query as input and returns search results with titles and URLs
- Optionally filter results by allowed or blocked domains
- The query must be at least 2 characters
- You may provide either allowed_domains OR blocked_domains, but NOT both
- Domains should be provided as a JSON array of strings
- This tool is read-only and does not modify any files

Usage: web_search <query> [--allowed-domains JSON] [--blocked-domains JSON]

Positional:
  query               The search query (at least 2 characters).

Options:
  --allowed-domains JSON    JSON array of domains to restrict results to.
  --blocked-domains JSON    JSON array of domains to exclude from results.

Example: web_search "React documentation" --allowed-domains '["react.dev", "github.com"]'

Response: OK | results:N | query:<text> | tokens:N
	Results follow: title | url | snippet (one per line, max 30).
Fails when: query too short (<2 chars), no results, network error.
If fails: broaden query, remove domain filters, check network connectivity.
After results: web_fetch the most relevant URL for detailed content.
Good: relevant results with useful URLs and snippets. Bad: no results (broaden query), irrelevant results (use --allowed-domains).
Don't use for: fetching specific URLs (use web_fetch), reading local files (use read).
Output example: OK | results:3 | query:"React docs" | tokens:60
  React Documentation | https://react.dev | Official React documentation...
  React GitHub | https://github.com/facebook/react | A JavaScript library...
Tip: use --allowed-domains to narrow results and reduce noise. Max 30 results; use specific queries to avoid broad matches.
Typical: web_search "React documentation"`,
	contextRequirements: (ctx) => ctx.diracWebToolsEnabled === true,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for web_search.",
			usage: '"latest developments in AI" --allowed-domains \'["example.com"]\'',
		},
	],
	metadata: {
		tags: ["web", "search", "query"],
		category: "web",
		concurrency: "parallel-safe",
		safety: ["network"],
		outputSize: "medium",
		llmsBrief: "Search the web for information",
		compactionSafety: "summarizable",
	},
}
