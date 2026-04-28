import { name, publisher, version } from "../package.json"
import { HostProvider } from "./hosts/host-provider"

const prefix = name === "claude-dev" || name === "dirac" ? "dirac" : name

/**
 * The registry info for the CLI, including its ID, name, version
 */
export const ExtensionRegistryInfo = {
	id: publisher + "." + name,
	name,
	version,
	publisher,
}

/**
 * @deprecated This interface is kept for backwards compatibility but host info
 * is now obtained dynamically from the HostBridge service.
 */
export interface HostInfo {
	/**
	 * The name of the host platform, e.g VSCode, IntelliJ Ultimate Edition, etc.
	 */
	platform: string
	/**
	 * The operating system platform, e.g. linux, darwin, win32
	 */
	os: string
	/**
	 * The type of the dirac host environment, e.g. 'CLI', 'Standalone'
	 */
	ide: string
	/**
	 * A distinct ID for this installation of the host client
	 */
	distinctId: string
	/**
	 * The version of the host platform
	 */
	hostVersion?: string
	/**
	 * The version of Dirac that the host client is running
	 */
	extensionVersion: string
}

let hostInfo = null as HostInfo | null

export const HostRegistryInfo = {
	init: async (distinctId: string) => {
		const host = await HostProvider.env.getHostVersion({})
		const hostVersion = host.version
		const extensionVersion = host.diracVersion || ExtensionRegistryInfo.version
		const platform = host.platform || "unknown"
		const os = process.platform || "unknown"
		const ide = host.diracType || "unknown"
		hostInfo = { hostVersion, extensionVersion, platform, os, ide, distinctId }
	},
	get: () => hostInfo,
}