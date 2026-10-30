export interface SidebarPreferences {
  created: boolean;
  subscribed: boolean;
}
const key = 'cumulus.sidebar-playlists.v1';
export function sidebarPreferences(storage?: Pick<Storage, 'getItem'>): SidebarPreferences {
  try {
    const value = JSON.parse((storage ?? localStorage).getItem(key) || '{}');
    return { created: value?.created !== false, subscribed: value?.subscribed !== false };
  } catch {
    return { created: true, subscribed: true };
  }
}
export function saveSidebarPreferences(value: SidebarPreferences) {
  localStorage.setItem(
    key,
    JSON.stringify({ created: value.created, subscribed: value.subscribed }),
  );
}
