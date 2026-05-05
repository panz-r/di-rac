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
export interface HostInfo {}

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