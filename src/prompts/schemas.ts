export const MANIFEST_SCHEMAS: Record<string, string> = {
	v1: `Output a JSON object with property "edits": an array of objects with fields:
- file: string (relative path)
- anchor: string (the line anchor from the file, e.g. "a3")
- newText: string (the replacement content)`,
	"v1.1": `Same as v1, but you may also include an optional "metadata" object with "version" field.`,
}
