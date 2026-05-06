import { name, publisher, version } from "../package.json"

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