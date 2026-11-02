import type { CatalogClient, CatalogResource } from './catalog.ts';

/** A fresh observation must identify both the account and requested resource. */
export async function catalogMembershipObservation(
  core: Pick<CatalogClient, 'catalogPage'>,
  epoch: string,
  resource: CatalogResource,
): Promise<boolean | null> {
  const kind = resource.kind === 'artist' ? 'artist_dynamic' : 'album';
  const result = await core.catalogPage(epoch, { kind, id: resource.id }, true);
  if (result.epoch !== epoch) throw { code: 'stale', message: '账号已切换' };
  const view = result.value;
  if (!view || view.query.kind !== kind || view.query.id !== resource.id)
    throw { code: 'stale', message: '收藏状态已变化，请重新读取' };
  if (resource.kind === 'artist' && view.page.kind === 'artist_dynamic') return view.page.followed;
  if (resource.kind === 'album' && view.page.kind === 'album' && view.page.album.id === resource.id)
    return view.page.album.subscribed;
  throw { code: 'stale', message: '收藏状态已变化，请重新读取' };
}
