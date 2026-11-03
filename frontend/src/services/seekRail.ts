import type { Timeline } from './player.ts';

/** Native trial bounds stay on the full-song clock when its total is known. */
export function seekRail(timeline: Timeline | null | undefined) {
  const start = timeline?.start ?? 0;
  const end = timeline?.end ?? 1;
  const total = timeline?.total;
  const trial =
    timeline?.mode === 'trial' &&
    timeline.end !== null &&
    Number.isFinite(start) &&
    Number.isFinite(end) &&
    start >= 0 &&
    end > start
      ? { start, end }
      : null;
  const span =
    trial && total != null && Number.isFinite(total) && total >= end
      ? { left: (100 * start) / total, right: (100 * end) / total }
      : null;
  return { min: span ? 0 : start, max: span ? total! : end, trial, span };
}

export function insidePlayableTrial(
  timeline: Timeline | null | undefined,
  seconds: number,
): boolean {
  if (!Number.isFinite(seconds)) return false;
  if (timeline?.mode !== 'trial') return true;
  const { trial } = seekRail(timeline);
  return !!trial && seconds >= trial.start && seconds <= trial.end;
}
