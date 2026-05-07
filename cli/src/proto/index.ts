/**
 * Lightweight proto stubs for CLI.
 * Replaces @shared/proto to eliminate the @bufbuild/protobuf dependency.
 * Only exports interfaces and simple create() functions — no wire serialization.
 */

export * as dirac from "./dirac"
export * as host from "./host"
