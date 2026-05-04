/**
 * Checks if the given email belongs to a Dirac bot user.
 * E.g. Emails ending with @dirac.run
 */
export function isDiracBotUser(email: string): boolean {
	return email.endsWith("@dirac.run")
}
