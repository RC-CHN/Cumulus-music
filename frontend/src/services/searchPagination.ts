/** Exact offsets followed for this category and search visit. Undefined is page one. */
export interface SearchPageStep {
  readonly offset: number;
  readonly number: number;
  readonly previous: SearchPageStep | undefined;
}

export function advanceSearchPage(
  current: SearchPageStep | undefined,
  nextOffset: number,
): SearchPageStep | undefined {
  if (
    !Number.isSafeInteger(nextOffset) ||
    nextOffset <= (current?.offset ?? 0) ||
    nextOffset > 10000
  )
    return undefined;
  return { offset: nextOffset, number: (current?.number ?? 1) + 1, previous: current };
}
