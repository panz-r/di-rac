enum ExtensionMode {
	Production = 1,
	Development = 2,
	Test = 3,
}

export interface DiracExtensionContext {
	readonly subscriptions: { dispose(): any }[]
	readonly extensionUri: string
	readonly extensionPath: string
	readonly environmentVariableCollection: any
	asAbsolutePath(relativePath: string): string
	readonly storageUri: string | undefined
	readonly storagePath: string | undefined
	readonly globalStorageUri: string
	readonly globalStoragePath: string
	readonly logUri: string
	readonly logPath: string
	readonly extensionMode: ExtensionMode
	readonly extension: Extension<any>
}

export interface Extension<T> {
	readonly id: string
	readonly extensionUri: string
	readonly extensionPath: string
	readonly isActive: boolean
	readonly packageJSON: any
	extensionKind: number
	readonly exports: T
	activate(): Promise<T>
}

export { ExtensionMode }
