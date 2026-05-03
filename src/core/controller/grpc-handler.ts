import { GrpcRequestRegistry } from "@/core/controller/grpc-request-registry"
import { ExtensionMessage } from "@/shared/ExtensionMessage"

export type StreamingResponseHandler<TResponse> = (
	response: TResponse,
	isLast?: boolean,
	sequenceNumber?: number,
) => Promise<void>

export type PostMessageToWebview = (message: ExtensionMessage) => Promise<boolean | undefined>

// Registry to track active gRPC requests and their cleanup functions
const requestRegistry = new GrpcRequestRegistry()

export function getRequestRegistry(): GrpcRequestRegistry {
	return requestRegistry
}
