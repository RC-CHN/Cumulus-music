export interface PlaylistDraft {
  name: string;
  description: string;
  tags: string[];
}
export type PlaylistField = keyof PlaylistDraft;

const fields = ['name', 'description', 'tags'] as const;
function comparable(field: PlaylistField, value: PlaylistDraft) {
  if (field === 'name') return value.name.trim();
  if (field === 'tags') return JSON.stringify(value.tags);
  return value.description;
}

/** Refresh server-only edits without replacing the user's draft. */
export function rebasePlaylistDraft(
  before: PlaylistDraft,
  draft: PlaylistDraft,
  latest: PlaylistDraft,
) {
  const next: PlaylistDraft = {
    name: latest.name,
    description: latest.description,
    tags: [...latest.tags],
  };
  const conflicts: PlaylistField[] = [];
  for (const field of fields) {
    const old = comparable(field, before);
    const local = comparable(field, draft);
    const remote = comparable(field, latest);
    if (old === local) continue;
    if (field === 'tags') next.tags = [...draft.tags];
    else next[field] = draft[field];
    if (old !== remote && local !== remote) conflicts.push(field);
  }
  return { draft: next, conflicts };
}
