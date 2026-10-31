/** A missing artist is metadata loss, while no item is an empty player. */
export function artistLabel(
  item: { artists: readonly string[] } | null | undefined,
  emptyText = '',
) {
  return item
    ? item.artists.filter((name) => name.trim().length > 0).join(' / ') || '未知歌手'
    : emptyText;
}
