// Replaces proto-generated host/env types with plain TypeScript.

import type { Metadata } from "../dirac/common"

// Re-export common types for the barrel.
export { EmptyRequest, Empty } from "../dirac/common"
export { String } from "../dirac/common"

export enum Setting {
    UNSUPPORTED = 0,
    ENABLED = 1,
    DISABLED = 2,
}

// --- GetHostVersion ---

export interface GetHostVersionResponse {
    platform?: string
    version?: string
    diracType?: string
    diracVersion?: string
}
export const GetHostVersionResponse = {
    create(o: Partial<GetHostVersionResponse> = {}): GetHostVersionResponse {
        return { ...o }
    },
}

// --- GetTelemetrySettings ---

export interface GetTelemetrySettingsResponse {
    isEnabled: Setting
    errorLevel?: string
}
export const GetTelemetrySettingsResponse = {
    create(o: Partial<GetTelemetrySettingsResponse> = {}): GetTelemetrySettingsResponse {
        return { isEnabled: o.isEnabled ?? Setting.UNSUPPORTED, ...o }
    },
}

// --- TelemetrySettingsEvent ---

export interface TelemetrySettingsEvent {
    isEnabled: Setting
    errorLevel?: string
}
export const TelemetrySettingsEvent = {
    create(o: Partial<TelemetrySettingsEvent> = {}): TelemetrySettingsEvent {
        return { isEnabled: o.isEnabled ?? Setting.UNSUPPORTED, ...o }
    },
}
