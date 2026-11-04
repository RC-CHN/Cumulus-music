export interface SidebarPreferences {
  created: boolean;
  subscribed: boolean;
}

export interface SidebarPlaylistMove {
  group: 'created' | 'subscribed';
  ids: string[];
  id: string;
  boundary: number;
}

export function sidebarMoveOrder(move: SidebarPlaylistMove): string[] | undefined {
  const { ids, id, boundary } = move;
  const from = ids.indexOf(id);
  if (from < 0 || !Number.isInteger(boundary) || boundary < 0 || boundary > ids.length) return;
  const to = boundary - Number(from < boundary);
  if (from === to) return;
  const next = [...ids];
  next.splice(from, 1);
  next.splice(to, 0, id);
  return next;
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
