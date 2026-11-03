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

/** Optional metadata marker; it never alters the authorization or seek range. */
export function chorusMarkerPercent(
  timeline: Timeline | null | undefined,
  seconds: number | null | undefined,
): number | null {
  if (!timeline || timeline.mode === 'cue' || seconds == null || !Number.isFinite(seconds))
    return null;
  const { min, max, trial } = seekRail(timeline);
  if (
    timeline.end == null ||
    !Number.isFinite(max) ||
    max <= min ||
    seconds <= 0 ||
    seconds < min ||
    seconds >= max ||
    (trial && seconds >= trial.end)
  )
    return null;
  return (100 * (seconds - min)) / (max - min);
}

/** Render only native buffered-ahead data on the currently authorized clock. */
export function bufferedSeekSpan(
  timeline: Timeline | null | undefined,
  position: number | null | undefined,
  bufferedThrough: number | null | undefined,
) {
  if (!timeline || timeline.end === null || position == null || bufferedThrough == null)
    return null;
  const { min, max } = seekRail(timeline);
  if (
    ![timeline.start, timeline.end, min, max, position, bufferedThrough].every(Number.isFinite) ||
    max <= min
  )
    return null;
  const start = Math.max(min, timeline.start, position);
  const end = Math.min(max, timeline.end, bufferedThrough);
  if (end <= start) return null;
  return { left: (100 * (start - min)) / (max - min), right: (100 * (end - min)) / (max - min) };
}
