export type AppearancePreference = 'system' | 'light' | 'dark';
export const appearanceKey = 'cumulus.appearance.v1';
export function appearancePreference(value: unknown): AppearancePreference {
  return value === 'light' || value === 'dark' ? value : 'system';
}
export function resolvedAppearance(preference: AppearancePreference, systemDark: boolean) {
  return preference === 'system' ? (systemDark ? 'dark' : 'light') : preference;
}
