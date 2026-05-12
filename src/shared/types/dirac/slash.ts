// Replaces proto-generated dirac/slash types with plain TypeScript.

import type { EmptyRequest } from "./common"
import type { Empty } from "./common"

export interface SlashCommandInfo {
	name: string
	description: string
	section: string
	cliCompatible: boolean
}
export const SlashCommandInfo = {
	create(o: Partial<SlashCommandInfo> = {}): SlashCommandInfo {
		return {
			name: o.name ?? "",
			description: o.description ?? "",
			section: o.section ?? "",
			cliCompatible: o.cliCompatible ?? false,
		}
	},
}

export interface SlashCommandsResponse {
	commands: SlashCommandInfo[]
}
export const SlashCommandsResponse = {
	create(o: Partial<SlashCommandsResponse> = {}): SlashCommandsResponse {
		return { commands: o.commands ?? [] }
	},
}
