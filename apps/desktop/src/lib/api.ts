import { invoke, isTauri } from '@tauri-apps/api/core';
export async function request<T>(method: string, params: unknown = {}): Promise<T> {
    if (!isTauri())
        throw new Error('Browser preview is disconnected. Open the desktop app to connect your local engine.');
    return invoke<T>('engine_request', { method, params });
}
