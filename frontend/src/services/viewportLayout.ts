// A compositor can stop issuing frames while the WebView still runs JS.
const frameDeadlineMs = 64;
const animationDeadlineMs = 450;

/** Prefer real layout frames, but never hold a viewport restore indefinitely. */
export function waitForViewportFrames(count: 1 | 2 = 1): Promise<void> {
  return new Promise((resolve) => {
    let remaining: number = count;
    let frame: number | undefined;
    let finished = false;
    const timer = setTimeout(finish, frameDeadlineMs);

    function finish() {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      if (frame !== undefined) cancelAnimationFrame(frame);
      resolve();
    }
    function tick() {
      if (finished) return;
      frame = undefined;
      if (--remaining === 0) finish();
      else frame = requestAnimationFrame(tick);
    }
    frame = requestAnimationFrame(tick);
  });
}

/** Wait for finite entry animations without relying on their finished promises. */
export async function waitForViewportAnimations(element: Element | null | undefined) {
  const animations = (element?.getAnimations({ subtree: true }) ?? []).filter((animation) => {
    const end = animation.effect?.getComputedTiming().endTime;
    return (
      typeof end === 'number' &&
      Number.isFinite(end) &&
      (animation.pending || animation.playState === 'running')
    );
  });
  if (!animations.length) return;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      Promise.allSettled(animations.map((animation) => animation.finished)),
      new Promise<void>((resolve) => {
        timer = setTimeout(resolve, animationDeadlineMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
