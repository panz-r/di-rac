import xxhashjs from "xxhashjs"

const { h32 } = xxhashjs

/**
 * 32-character base32 alphabet: digits + lowercase a-v.
 * Every 3-char code fits in a single LLM token across BBPE/SentencePiece tokenizers.
 * Provides 32,768 unique anchors (32^3), sufficient for files up to several thousand lines.
 */
const ALPHABET = "0123456789abcdefghijklmnopqrstuv"
const ALPHABET_LENGTH = ALPHABET.length

/** Default length of short hash codes. */
const DEFAULT_HASH_LENGTH = 3

/** Seed for xxHash32 (0 = default). */
const XXHASH_SEED = 0

/**
 * Returns the 32-character base32 alphabet used for short encoding.
 */
export function getAlphabet(): string {
	return ALPHABET
}

/**
 * Computes a 32-bit xxHash of the given string data.
 * Uses xxhashjs for pure JavaScript implementation.
 */
export function xxHash32(data: string): number {
	return h32(data, XXHASH_SEED).toNumber()
}

/**
 * Encodes a 32-bit hash into a short alphanumeric string of the given length.
 * Uses the base32 alphabet (digits + a-v).
 */
export function encodeShortHash(hash32: number, length: number = DEFAULT_HASH_LENGTH): string {
	let result = ""
	let remaining = hash32 >>> 0 // ensure unsigned
	for (let i = 0; i < length; i++) {
		result = ALPHABET[remaining % ALPHABET_LENGTH] + result
		remaining = Math.floor(remaining / ALPHABET_LENGTH)
	}
	return result
}

/**
 * Computes a content-based anchor hash for a single line.
 * Returns a 3-character code from the custom alphabet.
 *
 * Collision handling is done at the FileAnchorIndex level (per file),
 * where _N suffixes are appended if multiple lines map to the same 3-char code.
 */
export function computeLineHash(lineContent: string): string {
	const hash32 = xxHash32(lineContent)
	return encodeShortHash(hash32, DEFAULT_HASH_LENGTH)
}
