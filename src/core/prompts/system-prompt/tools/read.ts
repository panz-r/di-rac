import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.FILE_READ

export const read: DiracToolSpec = {
	id,
	name: "read",
	description: `Read files with detail levels: hint (kind+name only), preview (auto for large files), outline (defs with handles like fn:Name), skeleton (signatures only), full. Use --section fn:Name to jump to a symbol. Use --detail outline before --detail full for large files.

Example: read src/auth.ts --detail outline

Response: OK | detail:<level> | handles:N | lines:N | tokens:N. Content follows. Handles like fn:Name work with --section.
Note: --detail full auto-downgrades to preview for files over 50KB. Repeated reads at same detail are cached.
Fails when: file >50KB auto-downgrades, binary files show minimal content, --section not found (returns warning).
If fails: for large files use --detail outline first then --section or --range. For binary use bash file.
After results: if outline, use --section fn:Name to jump to body. If preview, use --range for specific lines.
Good: symbols visible, content at expected line, hash anchors stable. Bad: auto-downgraded (file too large, use --range), binary (use bash), section not found (use outline).
Don't use for: searching patterns (use search), code structure across files (use symbols or repo).
Output example (outline): OK | detail:outline | handles:3 | lines:42 | tokens:120
  fn:login (line 42)  fn:logout (line 58)  class:AuthService (line 10)
Typical: read src/file.ts --detail outline`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for read. Use --detail, --range, --section, --retry flags.",
			usage: "src/auth.ts --detail outline",
		},
	],
	metadata: {
		tags: ["file", "read", "code"],
		category: "file",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "large",
		llmsBrief: "Read files with detail levels",
		compactionSafety: "discardable",
	},
}
