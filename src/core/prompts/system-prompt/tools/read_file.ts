import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.FILE_READ

export const read_file: DiracToolSpec = {
	id,
	name: "read_file",
	description:
		'Reads file contents with structured detail levels. For large files (>50KB), it defaults to a preview. Use detail="outline" or detail="skeleton" to explore structure without full body tokens. Supports page-based navigation and jump-to-section. Returns hash-anchored lines for edit_file.',
	parameters: [
		{
			name: "paths",
			required: true,
			type: "array",
			items: { type: "string" },
			instruction: "An array of relative paths to the source files.",
			usage: '["src/utils/math.ts"]',
		},
		{
			name: "detail",
			required: false,
			type: "string",
			instruction: 'Detail level: "preview" (200 lines + symbols), "skeleton" (stripped bodies), "outline" (symbols only), or "full". Defaults to "full" for small files, "preview" for large.',
			usage: '"outline"',
		},
		{
			name: "max_tokens",
			required: false,
			type: "integer",
			instruction: "Optional budget. If set, detail level auto-degrades to stay within token limit.",
			usage: "1000",
		},
		{
			name: "page",
			required: false,
			type: "string",
			instruction: 'Navigation: "next" (next 200 lines), "prev" (previous 200 lines), or "section" (jump to structural handle).',
			usage: '"next"',
		},
		{
			name: "section",
			required: false,
			type: "string",
			instruction: 'Structural handle (e.g., "fn:myFunc") to jump to when page="section".',
			usage: '"fn:AuthService.login"',
		},
		{
			name: "start_line",
			required: false,
			type: "integer",
			instruction: "Optional. Line range start (1-based). Overrides pagination.",
			usage: "10",
		},
		{
			name: "end_line",
			required: false,
			type: "integer",
			instruction: "Optional. Line range end.",
			usage: "50",
		},
	],
}
