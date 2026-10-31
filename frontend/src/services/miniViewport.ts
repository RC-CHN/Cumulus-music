export interface MiniViewport {
  width: number;
  height: number;
  scale: number;
}
/** Geometry-only negotiation runs before the native handover, within its 8s
 * lifetime. It never retries playback or account operations. */
export async function prepareMiniViewport(
  ready: (viewport: MiniViewport) => Promise<boolean>,
  observe = (): MiniViewport => ({
    width: innerWidth,
    height: innerHeight,
    scale: devicePixelRatio,
  }),
  wait = () => new Promise<void>((resolve) => setTimeout(resolve, 25)),
) {
  for (let attempt = 0; attempt < 3; attempt++) {
    if (await ready(observe())) return;
    for (let frame = 0; frame < 20; frame++) {
      const current = observe();
      if (Math.abs(current.width - 420) <= 1 && Math.abs(current.height - 206) <= 1) break;
      await wait();
    }
  }
  throw new Error('迷你窗口尺寸尚未就绪，请从主窗口重新打开');
}
