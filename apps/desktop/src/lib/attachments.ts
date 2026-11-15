import type { FileInfo, Preview } from './protocol';

interface AttachmentHost {
    read: (key: string) => Preview[];
    write: (key: string, items: Preview[]) => void;
    pending: (key: string, count: number) => void;
    disposed: () => boolean;
    createPreview: (info: FileInfo) => string | undefined;
    revokePreview: (url: string) => void;
    removeStaged: (path: string) => Promise<unknown>;
    usedByOtherDraft?: (key: string, path: string) => boolean;
}

/** Every asynchronous operation owns the draft key captured before its first await. */
export function createAttachmentController(host: AttachmentHost) {
    const pending = new Map<string, number>();
    async function run<T>(key: string, operation: () => Promise<T>): Promise<T> {
        const count = (pending.get(key) ?? 0) + 1;
        pending.set(key, count);
        host.pending(key, count);
        try { return await operation(); }
        finally {
            const remaining = (pending.get(key) ?? 1) - 1;
            if (remaining) pending.set(key, remaining); else pending.delete(key);
            host.pending(key, remaining);
        }
    }
    async function add(key: string, info: FileInfo, staged = false) {
        // Duplicate canonical paths do not consume another slot or byte budget.
        if (host.read(key).some(item => item.path === info.path)) return;
        let preview: string | undefined;
        try {
            if (host.disposed()) throw new Error('The draft was closed before the attachment was ready.');
            const items = host.read(key);
            if (items.length >= 12) throw new Error('Attach up to 12 unique files.');
            if (info.size > 16 * 1024 * 1024) throw new Error('Each attachment must be no larger than 16 MiB.');
            if (items.reduce((sum, item) => sum + item.size, 0) + info.size > 40 * 1024 * 1024)
                throw new Error('Attachments must total no more than 40 MiB.');
            preview = host.createPreview(info);
            host.write(key, [...items, {
                id: crypto.randomUUID(), path: info.path, kind: info.kind, mime: info.mime,
                name: info.name, size: info.size, preview, staged,
            }]);
        } catch (error) {
            if (preview) host.revokePreview(preview);
            if (staged) {
                try { await host.removeStaged(info.path); }
                catch { throw new Error(`${String(error)} The unused image could not be cleaned up; it was retained safely.`); }
            }
            throw error;
        }
    }
    async function remove(key: string, id: string) {
        await run(key, async () => {
            const item = host.read(key).find(item => item.id === id);
            if (!item) return;
            // The native command enforces app ownership and checks durable references.
            if (item.staged && !host.usedByOtherDraft?.(key, item.path)) await host.removeStaged(item.path);
            if (item.preview) host.revokePreview(item.preview);
            host.write(key, host.read(key).filter(candidate => candidate.id !== id));
        });
    }
    return { run, add, remove, isPending: (key: string) => (pending.get(key) ?? 0) > 0 };
}
