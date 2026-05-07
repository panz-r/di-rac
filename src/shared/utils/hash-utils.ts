import xxhashjs from "xxhashjs"

const { h32 } = xxhashjs

/** 32-character base32 alphabet: digits + lowercase a-v. */
const ALPHABET = "0123456789abcdefghijklmnopqrstuv"
const ALPHABET_LENGTH = ALPHABET.length
const DEFAULT_HASH_LENGTH = 3
const XXHASH_SEED = 0

export function getAlphabet(): string {
	return ALPHABET
}

export function xxHash32(data: string): number {
	return h32(data, XXHASH_SEED).toNumber()
}

export function encodeShortHash(hash32: number, length: number = DEFAULT_HASH_LENGTH): string {
	let result = ""
	let remaining = hash32 >>> 0
	for (let i = 0; i < length; i++) {
		result = ALPHABET[remaining % ALPHABET_LENGTH] + result
		remaining = Math.floor(remaining / ALPHABET_LENGTH)
	}
	return result
}

export function computeLineHash(lineContent: string): string {
	const hash32 = xxHash32(lineContent)
	return encodeShortHash(hash32, DEFAULT_HASH_LENGTH)
}