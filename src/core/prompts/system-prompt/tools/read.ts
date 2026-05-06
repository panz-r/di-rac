import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.FILE_READ

export const read: DiracToolSpec = {
	id,
	name: "read",
	description: `Read files with detail levels: hint (kind+name only), preview (auto for large files), outline (defs with handles like fn:Name), skeleton (signatures only), full. Use --section fn:Name to jump to a symbol. Use --detail outline before --detail full for large files.

Examples:
  read src/auth.ts --detail hint
  read src/auth.ts --detail outline
  read src/auth.ts src/db.ts --detail skeleton
  read src/auth.ts --range "1-50,200-250"
  read src/auth.ts --section fn:login

Response: OK | detail:<level> | handles:N | lines:N | tokens:N. Content follows. Handles like fn:Name work with --section.
Note: --detail full auto-downgrades to preview for files over 50KB. Response starts with TRUNCATED if so.
Note: repeated reads of the same file at the same detail level are cached and instant.
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
