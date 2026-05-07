declare module "xxhashjs" {
	interface XXHash32 {
		toNumber(): number
	}

	interface H32 {
		(data: string, seed?: number): XXHash32
	}

	interface XXHashjs {
		h32: H32
	}

	const xxhashjs: XXHashjs
	export = xxhashjs
}