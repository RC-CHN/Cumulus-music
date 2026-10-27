import type { CoverCrop } from './playlistCover.ts';
export interface CropPosition {
  x: number;
  y: number;
  zoom: number;
}
export const coverViewport = { width: 362, height: 260, size: 200 };
const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));
/** M2167: horizontal-cover, 200px crop, zoom initially1, min covers both axes,
 * max10. Coordinates returned to Rust always refer to the oriented original. */
export function coverZoomLimits(width: number, height: number) {
  const min = (200 * width) / (362 * Math.min(width, height));
  return { min, max: Math.max(10, min) };
}
export function coverPosition(width: number, height: number, value?: CropPosition): CropPosition {
  const limits = coverZoomLimits(width, height);
  const zoom = clamp(value?.zoom ?? 1, limits.min, limits.max);
  const half = 100 / ((362 / width) * zoom);
  return {
    x: clamp(value?.x ?? width / 2, half, width - half),
    y: clamp(value?.y ?? height / 2, half, height - half),
    zoom,
  };
}
export function restoreCoverPosition(width: number, height: number, crop: CoverCrop): CropPosition {
  return coverPosition(width, height, {
    x: crop.x + crop.size / 2,
    y: crop.y + crop.size / 2,
    zoom: (200 * width) / (362 * crop.size),
  });
}
export function coverCrop(width: number, height: number, position: CropPosition): CoverCrop {
  const p = coverPosition(width, height, position);
  const size = clamp(Math.round(200 / ((362 / width) * p.zoom)), 1, Math.min(width, height));
  return {
    x: clamp(Math.round(p.x - size / 2), 0, width - size),
    y: clamp(Math.round(p.y - size / 2), 0, height - size),
    size,
  };
}
export function dragCover(
  width: number,
  height: number,
  position: CropPosition,
  dx: number,
  dy: number,
) {
  const scale = (362 / width) * position.zoom;
  return coverPosition(width, height, {
    ...position,
    x: position.x - dx / scale,
    y: position.y - dy / scale,
  });
}
