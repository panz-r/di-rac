import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.FILE_READ

export const read_file: DiracToolSpec = {
	id,
	name: "read_file",
	description: `Reads file contents with structured detail levels. For large files (>50KB), defaults to a preview.

Usage: read_file <path>... [options]

Positional:
  path            One or more file paths to read (relative to CWD)

Options:
  --detail LEVEL  Detail level: preview, skeleton, outline, full.
                  Defaults to "full" for small files, "preview" for large.
  --start-line N  Start reading from line N (1-based). Overrides pagination.
  --end-line N    Stop reading at line N.
  --max-tokens N  Token budget. If set, detail level auto-degrades to fit.
  --page DIR      Navigation: next (next 200 lines), prev (previous 200 lines),
                  or section (jump to structural handle).
  --section ID    Structural handle to jump to when page=section (e.g. fn:myFunc).
  --ranges JSON   Non-contiguous line ranges. Takes precedence over start/end-line.

Examples:
  read_file src/auth.py --detail outline
  read_file src/auth.py src/db.py --start-line 10 --end-line 50
  read_file src/auth.py --ranges '[{"start":1,"end":50},{"start":100,"end":150}]'
  read_file src/auth.py --page next
  read_file src/auth.py --section fn:AuthService.login

Returns hash-anchored lines for edit_file.`,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for read_file.",
			usage: 'src/auth.py --detail outline --start-line 10 --end-line 50',
		},
	],
	metadata: {
		tags: ["file", "read"],
		category: "file-io",
		concurrency: "parallel-safe",
		safety: ["read"],
		outputSize: "large",
		llmsBrief: "Read file contents with line numbers and range support",
		compactionSafety: "summarizable",
	},
}
