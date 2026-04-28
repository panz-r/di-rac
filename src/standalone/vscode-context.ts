import * as fs from "node:fs"
import * as path from "path"
import { log } from "./utils"

const SETTINGS_SUBFOLDER = "data"

export function initializeContext(diracDir?: string) {
	const dirac = diracDir || process.env.DIRAC_DIR || `${require("os").homedir()}/.dirac`
	const DATA_DIR = path.join(dirac, SETTINGS_SUBFOLDER)
	const INSTALL_DIR = process.env.INSTALL_DIR || __dirname
	const EXTENSION_DIR = path.join(INSTALL_DIR, "extension")

	fs.mkdirSync(DATA_DIR, { recursive: true })
	log("Using settings dir:", DATA_DIR)

	const extensionContext = {
		extensionUri: EXTENSION_DIR,
		extensionPath: EXTENSION_DIR,
		storageUri: path.join(DATA_DIR, "workspaceStorage"),
		storagePath: path.join(DATA_DIR, "workspaceStorage"),
		globalStorageUri: DATA_DIR,
		globalStoragePath: DATA_DIR,
		logUri: path.join(dirac, "logs"),
		logPath: path.join(dirac, "logs"),
		extensionMode: 1 as const,
		extension: {
			id: "dirac.standalone",
			extensionUri: EXTENSION_DIR,
			extensionPath: EXTENSION_DIR,
			isActive: true,
			packageJSON: { version: "0.0.0" },
			extensionKind: 2,
			exports: {},
			activate: () => Promise.resolve({}),
		},
		environmentVariableCollection: { persistent: false },
		asAbsolutePath: (relativePath: string) => path.join(EXTENSION_DIR, relativePath),
		subscriptions: [] as { dispose(): any }[],
	}

	log("Finished loading standalone context...")

	return { extensionContext, DATA_DIR, EXTENSION_DIR }
}
