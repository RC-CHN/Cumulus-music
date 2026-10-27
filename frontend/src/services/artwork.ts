/** Resize a core-approved public CDN image; no media authorization enters the UI. */
export function artworkAt(url: string, pixels: number): string {
  const image = new URL(url);
  if (
    image.protocol !== 'https:' ||
    !['music.126.net', 'music.163.com'].some((domain) => image.hostname.endsWith(`.${domain}`))
  )
    return url;
  image.searchParams.set('param', `${pixels}y${pixels}`);
  return image.href;
}
