declare module 'xxhashjs' {
	export interface XXHash<T> {
		update(data: string | Buffer): XXHash<T>
		digest(): T
	}
	export const h32: (data: string | Buffer, seed: number) => { toNumber(): number }
	export default { h32 }
}
