type SuggestionKey = 'ArrowDown' | 'ArrowUp' | 'Home' | 'End';

/** The suggestion list is cyclic; an unfocused list starts at the matching edge. */
export function searchSuggestionIndex(count: number, current: number, key: SuggestionKey) {
  if (!count) return undefined;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  if (current < 0) return key === 'ArrowDown' ? 0 : count - 1;
  return (current + (key === 'ArrowDown' ? 1 : count - 1)) % count;
}
