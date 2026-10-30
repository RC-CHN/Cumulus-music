export type LibraryMode = 'grid' | 'list';
export type LibraryGroup = 'created' | 'subscribed';
export interface LibraryViewState {
  createdMode: LibraryMode;
  subscribedMode: LibraryMode;
  createdPage: number;
  subscribedPage: number;
  scrollTop: number;
  focusId: string;
}
const key = 'cumulus.playlist-library-layout.v1';
export function libraryView(storage?: Pick<Storage, 'getItem'>): LibraryViewState {
  let value: Partial<LibraryViewState> = {};
  try {
    value = JSON.parse((storage ?? localStorage).getItem(key) || '{}') ?? {};
  } catch {
    /* Recover invalid presentation preferences. */
  }
  return {
    createdMode: value.createdMode === 'list' ? 'list' : 'grid',
    subscribedMode: value.subscribedMode === 'list' ? 'list' : 'grid',
    createdPage: 1,
    subscribedPage: 1,
    scrollTop: 0,
    focusId: '',
  };
}
export function saveLibraryView(state: LibraryViewState, storage: Pick<Storage, 'setItem'>) {
  // Only presentation settings survive a restart; no account/resource identifiers.
  storage.setItem(
    key,
    JSON.stringify({ createdMode: state.createdMode, subscribedMode: state.subscribedMode }),
  );
}
export function libraryPage(page: number, count: number) {
  return Math.max(1, Math.min(page, Math.ceil(count / 20)));
}
