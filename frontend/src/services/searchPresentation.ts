import type { SearchPresentationPatch, SearchResultView } from './search.ts';
import { withoutSearchFooters } from './searchViewport.ts';

/** Merge only presentation fields into the newest page, retaining privileges. */
export function patchSearchPresentation(
  current: SearchResultView,
  patch: SearchPresentationPatch,
): SearchResultView | undefined {
  if (
    current.query.category !== 'all' ||
    current.presentationReceipt !== patch.receipt ||
    ![null, 'compact_grid', 'carousel', 'compact_carousel'].includes(patch.previewLayout) ||
    patch.sections.length > 50
  )
    return;
  const seen = new Set<string>();
  for (const section of patch.sections) {
    const source = current.sections.find((item) => item.id === section.id);
    if (!source || seen.has(section.id) || section.items.length > 6) return;
    seen.add(section.id);
    const indices = new Set<number>();
    for (const row of section.items) {
      const item = source.items[row.index];
      if (
        !Number.isInteger(row.index) ||
        row.index < 0 ||
        indices.has(row.index) ||
        item?.kind !== 'song' ||
        item.track.id !== row.trackId
      )
        return;
      indices.add(row.index);
    }
  }
  const safe = withoutSearchFooters(current);
  return {
    ...safe,
    presentationRevision: patch.revision,
    previewLayout: patch.previewLayout ?? undefined,
    sections: safe.sections.map((section) => {
      const updates = patch.sections.find((item) => item.id === section.id);
      if (!updates) return section;
      return {
        ...section,
        items: section.items.map((item, index) => {
          const row = updates.items.find((item) => item.index === index);
          return row && item.kind === 'song'
            ? { ...item, hint: row.hint ?? undefined, reasons: row.reasons }
            : item;
        }),
      };
    }),
  };
}
