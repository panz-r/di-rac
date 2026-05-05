import { Logger } from "@/shared/services/Logger"
import { DiracError } from "./DiracError"
import { IErrorProvider } from "./providers/IErrorProvider"

/**
 * Supported error provider types
 */
export type ErrorProviderType = "dirac" | "no-op"

/**
 * Configuration for error providers
 */
export interface ErrorProviderConfig {
	type: ErrorProviderType
}

/**
 * Factory class for creating error providers
 * Allows easy switching between different error tracking providers
 */
export class ErrorProviderFactory {
	/**
	 * Creates an error provider based on the provided configuration
	 * @param config Configuration for the error provider
	 * @returns IErrorProvider instance
	 */
	public static async createProvider(config: ErrorProviderConfig): Promise<IErrorProvider> {
		switch (config.type) {
			case "dirac":
			default:
				return new NoOpErrorProvider()
		}
	}

	/**
	 * Gets the default error provider configuration
	 * @returns Default configuration - no-op since telemetry is removed
	 */
	public static getDefaultConfig(): ErrorProviderConfig {
		return {
			type: "no-op",
		}
	}
}

/**
 * No-operation error provider for when error logging is disabled
 * or for testing purposes
 */
class NoOpErrorProvider implements IErrorProvider {
	async captureException(error: Error | DiracError, properties?: Record<string, unknown>): Promise<void> {
		Logger.error("[NoOpErrorProvider] captureException called", { error: error.message || String(error), properties })
	}

	public logException(error: Error | DiracError, _properties?: Record<string, unknown>): void {
		// Use Logger.error directly to avoid potential infinite recursion through Logger
		Logger.error("[NoOpErrorProvider]", error.message || String(error))
	}

	public logMessage(
		message: string,
		level: "error" | "warning" | "log" | "debug" | "info" = "log",
		properties: Record<string, unknown> = {},
	): void {
		Logger.log("[NoOpErrorProvider]", { message, level, properties })
	}

	public isEnabled(): boolean {
		return true
	}

	public getSettings() {
		return {
			enabled: true,
			hostEnabled: true,
			level: "all" as const,
		}
	}

	public async dispose(): Promise<void> {
		Logger.info("[NoOpErrorProvider] Disposing")
	}
}
