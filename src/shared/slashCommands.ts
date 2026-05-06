export interface SlashCommand {
	name: string
	description?: string
	section?: "default" | "custom" | "skill"
	cliCompatible?: boolean
}

export const BASE_SLASH_COMMANDS: SlashCommand[] = [
	{
		name: "newtask",
		description: "Create a new task with context from the current task",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "smol",
		description: "Condenses your current context window",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "newrule",
		description: "Create a new rule based on your conversation",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "reportbug",
		description: "Create a Github issue with divr",
		section: "default",
		cliCompatible: true,
	},
]

// CLI-only slash commands (handled locally, not sent to backend)
export const CLI_ONLY_COMMANDS: SlashCommand[] = [
	{
		name: "help",
		description: "Learn how to use divr CLI",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "settings",
		description: "Change API provider, auto-approve, and feature settings",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "history",
		description: "Browse and search task history",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "clear",
		description: "Clear the current task and start fresh",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "exit",
		description: "Alternative to Ctrl+C",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "q",
		description: "Alternative to Ctrl+C",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "skills",
		description: "View and manage installed skills",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "steer/msg",
		description: "Queue a message for the next agent turn",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "steer/clear",
		description: "Clear queued steer messages",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "defer/msg",
		description: "Queue a message for after task completion",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "defer/clear",
		description: "Clear deferred messages",
		section: "default",
		cliCompatible: true,
	},
	{
		name: "observe",
		description: "Toggle observer agent on/off",
		section: "default",
		cliCompatible: true,
	},
]
