import { Logger } from "@/shared/services/Logger"

export interface PythonApi {
	environments: {
		getActiveEnvironmentPath(): { path: string }
		getEnvironmentVariables(): Promise<{ [key: string]: string | undefined }>
	}
}

export async function getPythonApi(): Promise<PythonApi | undefined> {
	return undefined
}

export async function getPythonEnvironmentVariables(): Promise<{ [key: string]: string | undefined } | undefined> {
	return undefined
}
