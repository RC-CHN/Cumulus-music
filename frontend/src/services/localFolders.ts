import type { LocalPage } from './local';

type Group = LocalPage['groups'][number];

function pathParts(path: string) {
  if (path.startsWith('/')) return ['posix', ...path.slice(1).split('/').filter(Boolean)];
  if (/^[a-z]:[/\\]/i.test(path))
    return [
      'drive',
      path.slice(0, 2),
      ...path
        .slice(3)
        .split(/[/\\]+/)
        .filter(Boolean),
    ];
  if (path.startsWith('\\\\')) {
    const [server, share, ...segments] = path
      .slice(2)
      .split(/[/\\]+/)
      .filter(Boolean);
    if (server && share) return ['unc', server, share, ...segments];
  }
  return null;
}

export interface FolderNode {
  group: Group;
  name: string;
  children: FolderNode[];
  parent: FolderNode | undefined;
}

/** Link only groups backed by an exact native folder query. Never create an
 * inferred parent that could accidentally turn playback into another scope. */
export function localFolderTree(groups: readonly Group[]): FolderNode[] {
  const byPath = new Map<string, FolderNode | null>();
  const entries = groups.map((group) => {
    const parts = pathParts(group.key);
    const node: FolderNode = { group, name: group.name, children: [], parent: undefined };
    if (parts) {
      const key = parts.join('\0');
      byPath.set(key, byPath.has(key) ? null : node);
    }
    return { node, parts };
  });
  const roots: FolderNode[] = [];
  for (const { node, parts } of entries) {
    const floor = parts?.[0] === 'unc' ? 3 : parts?.[0] === 'drive' ? 2 : 1;
    if (parts) {
      const name = parts.at(-1);
      while (parts.length > floor) {
        parts.pop();
        const parent = byPath.get(parts.join('\0'));
        if (!parent) continue;
        node.parent = parent;
        node.name = name ?? node.name;
        parent.children.push(node);
        break;
      }
    }
    if (!node.parent) roots.push(node);
  }
  return roots;
}

export function visibleLocalFolders(
  roots: readonly FolderNode[],
  expanded: ReadonlySet<string>,
): { node: FolderNode; depth: number }[] {
  const rows: { node: FolderNode; depth: number }[] = [];
  const pending = roots.map((node) => ({ node, depth: 0 })).reverse();
  while (pending.length) {
    const row = pending.pop()!;
    rows.push(row);
    if (!expanded.has(row.node.group.key)) continue;
    for (let index = row.node.children.length - 1; index >= 0; index--)
      pending.push({ node: row.node.children[index]!, depth: row.depth + 1 });
  }
  return rows;
}
