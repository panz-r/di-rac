import { userInfo } from "os"

export const WINDOWS_POWERSHELL_7_PATH = "C:\\Program Files\\PowerShell\\7\\pwsh.exe"
export const WINDOWS_POWERSHELL_LEGACY_PATH = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"

const SHELL_PATHS = {
	// Windows paths
	POWERSHELL_7: WINDOWS_POWERSHELL_7_PATH,
	POWERSHELL_LEGACY: WINDOWS_POWERSHELL_LEGACY_PATH,
	CMD: "C:\\Windows\\System32\\cmd.exe",
	WSL_BASH: "/bin/bash",
	GIT_BASH: "C:\\Program Files\\Git\\bin\\bash.exe",
	// Unix paths
	MAC_DEFAULT: "/bin/zsh",
	LINUX_DEFAULT: "/bin/bash",
	CSH: "/bin/csh",
	BASH: "/bin/bash",
	KSH: "/bin/ksh",
	SH: "/bin/sh",
	ZSH: "/bin/zsh",
	DASH: "/bin/dash",
	TCSH: "/bin/tcsh",
	FALLBACK: "/bin/sh",
} as const

// -----------------------------------------------------
// 1) General Fallback Helpers
// -----------------------------------------------------

function getShellFromUserInfo(): string | null {
	try {
		const { shell } = userInfo()
		return shell || null
	} catch {
		return null
	}
}

function getShellFromEnv(): string | null {
	const { env } = process

	if (process.platform === "win32") {
		return env.COMSPEC || "C:\\Windows\\System32\\cmd.exe"
	}

	if (process.platform === "darwin") {
		return env.SHELL || "/bin/zsh"
	}

	if (process.platform === "linux") {
		return env.SHELL || "/bin/bash"
	}
	return null
}

// -----------------------------------------------------
// 2) Terminal Profile Interface and Utilities
// -----------------------------------------------------

import { TerminalProfile } from "@shared/types/dirac/state"

export function getAvailableTerminalProfiles(): TerminalProfile[] {
	const profiles: TerminalProfile[] = [
		{
			id: "default",
			name: "Default",
			description: "Use the default terminal configuration",
		},
	]

	if (process.platform === "win32") {
		profiles.push(
			{
				id: "powershell-7",
				name: "PowerShell 7",
				path: SHELL_PATHS.POWERSHELL_7,
				description: "PowerShell 7 (pwsh.exe)",
			},
			{
				id: "powershell-legacy",
				name: "Windows PowerShell",
				path: SHELL_PATHS.POWERSHELL_LEGACY,
				description: "Windows PowerShell 5.x",
			},
			{
				id: "cmd",
				name: "Command Prompt",
				path: SHELL_PATHS.CMD,
				description: "Command Prompt (cmd.exe)",
			},
			{
				id: "wsl-bash",
				name: "WSL Bash",
				path: SHELL_PATHS.WSL_BASH,
				description: "Windows Subsystem for Linux Bash",
			},
			{
				id: "git-bash",
				name: "Git Bash",
				path: SHELL_PATHS.GIT_BASH,
				description: "Git Bash (bash.exe from Git for Windows)",
			},
		)
	} else if (process.platform === "darwin") {
		profiles.push(
			{
				id: "zsh",
				name: "zsh",
				path: SHELL_PATHS.ZSH,
				description: "Z shell (default on macOS)",
			},
			{
				id: "bash",
				name: "bash",
				path: SHELL_PATHS.BASH,
				description: "Bourne Again Shell",
			},
		)
	} else if (process.platform === "linux") {
		profiles.push(
			{
				id: "bash",
				name: "bash",
				path: SHELL_PATHS.BASH,
				description: "Bourne Again Shell (default on most Linux)",
			},
			{
				id: "zsh",
				name: "zsh",
				path: SHELL_PATHS.ZSH,
				description: "Z shell",
			},
			{
				id: "dash",
				name: "dash",
				path: SHELL_PATHS.DASH,
				description: "Debian Almquist Shell",
			},
		)
	}

	return profiles
}

export function getShellForProfile(profileId: string): string {
	if (profileId === "default") {
		return getShell()
	}

	const profiles = getAvailableTerminalProfiles()
	const profile = profiles.find((p) => p.id === profileId)

	if (profile?.path) {
		return profile.path
	}

	return getShell()
}

// -----------------------------------------------------
// 3) Publicly Exposed Shell Getter
// -----------------------------------------------------

export function getShell(): string {
	// 1. Try userInfo()
	const userInfoShell = getShellFromUserInfo()
	if (userInfoShell) {
		return userInfoShell
	}

	// 2. Try environment variable
	const envShell = getShellFromEnv()
	if (envShell) {
		return envShell
	}

	// 3. Platform-specific fallback
	if (process.platform === "win32") {
		return SHELL_PATHS.CMD
	}
	return SHELL_PATHS.FALLBACK
}
