import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.EXPAND_SYMBOL

export const expand_symbol: DiracToolSpec = {
	id,
	name: "expand_symbol",
	description:
		"Extracts the complete body of a symbol (function, class, method) using its structural handle (ID). Handles are stable IDs like 'fn:myFunc' or 'class:MyClass' obtained from read_file(detail='outline' or 'skeleton'). This is the most token-efficient way to read specific code blocks.",
	parameters: [
		{
			name: "path",
			required: true,
			type: "string",
			instruction: "Relative path to the source file.",
			usage: '"src/utils/math.ts"',
		},
		{
			name: "symbol",
			required: true,
			type: "string",
			instruction: "The structural handle (ID) of the symbol to expand.",
			usage: '"fn:AuthService.login"',
		},
	],
}
