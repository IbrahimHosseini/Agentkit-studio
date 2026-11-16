import type { PermissionMode } from './protocol';

export const permissionChoices: { value: PermissionMode; label: string; description: string }[] = [
    { value: 'ask', label: 'Ask for approval', description: 'Ask before running a writing plan or granting elevated access' },
    { value: 'auto', label: 'Approve for me', description: 'Run project work and let the provider review elevated access' },
    { value: 'full_access', label: 'Full access', description: 'Allow unrestricted file and internet access without approval prompts' },
];

export function permissionMode(value: unknown): PermissionMode {
    return value === 'auto' || value === 'full_access' ? value : 'ask';
}
