/**
 * Lightweight proto stubs for CLI — interfaces and create() only, no protobuf dependency.
 */

export interface Metadata {}

export interface EmptyRequest {}
export const EmptyRequest = {
	create(_: Partial<EmptyRequest> = {}): EmptyRequest { return {} },
}

export interface Empty {}
export const Empty = {
	create(_: Partial<Empty> = {}): Empty { return {} },
}

export interface StringRequest { value: string }
export const StringRequest = {
	create(o: Partial<StringRequest> = {}): StringRequest { return { value: o.value ?? "" } },
}

export interface StringArrayRequest { value: string[] }
export const StringArrayRequest = {
	create(o: Partial<StringArrayRequest> = {}): StringArrayRequest { return { value: o.value ?? [] } },
}

export interface String { value: string }
export const String = {
	create(o: Partial<{ value: string }> = {}): String { return { value: o.value ?? "" } },
}

export interface Int64Request { value: number }
export const Int64Request = {
	create(o: Partial<Int64Request> = {}): Int64Request { return { value: o.value ?? 0 } },
}

export interface Int64 { value: number }
export const Int64 = {
	create(o: Partial<Int64> = {}): Int64 { return { value: o.value ?? 0 } },
}

export interface BooleanRequest { value: boolean }
export const BooleanRequest = {
	create(o: Partial<BooleanRequest> = {}): BooleanRequest { return { value: o.value ?? false } },
}

export interface Boolean_ { value: boolean }
export const Boolean = {
	create(o: Partial<Boolean_> = {}): Boolean_ { return { value: o.value ?? false } },
}

export interface BooleanResponse { value: boolean }
export const BooleanResponse = {
	create(o: Partial<BooleanResponse> = {}): BooleanResponse { return { value: o.value ?? false } },
}

export interface StringArray { values: string[] }
export const StringArray = {
	create(o: Partial<StringArray> = {}): StringArray { return { values: o.values ?? [] } },
}

export interface KeyValuePair { key: string; value: string }
export const KeyValuePair = {
	create(o: Partial<KeyValuePair> = {}): KeyValuePair { return { key: o.key ?? "", value: o.value ?? "" } },
}

export interface GetTaskHistoryRequest {
	metadata: Metadata | undefined
	favoritesOnly: boolean
	searchQuery: string
	sortBy: string
	currentWorkspaceOnly: boolean
}
export const GetTaskHistoryRequest = {
	create(o: Partial<GetTaskHistoryRequest> = {}): GetTaskHistoryRequest {
		return {
			metadata: undefined,
			favoritesOnly: false,
			searchQuery: "",
			sortBy: "",
			currentWorkspaceOnly: false,
			...o,
		}
	},
}

export interface SlashCommandInfo {
	name: string
	description: string
	section: string
	cliCompatible: boolean
}

export enum RuleScope {
	LOCAL = 0,
	GLOBAL = 1,
	REMOTE = 2,
	UNRECOGNIZED = -1,
}
