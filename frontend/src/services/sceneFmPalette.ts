import type { AccountData } from './contracts';
export interface SceneFmColor {
  r: number | null;
  g: number | null;
  b: number | null;
}
export type SceneFmBackground =
  | { kind: 'theme_fallback' }
  | { kind: 'gradient'; first: SceneFmColor; middle: SceneFmColor };
export type SceneFmPaletteRead =
  | { kind: 'ready'; background: SceneFmBackground }
  | {
      kind: 'sample';
      receipt: string;
      mimeType: string;
      bytes: number[];
      width: number;
      height: number;
    };
export interface SceneFmPaletteClient {
  sceneFmPaletteRead(
    epoch: string,
    revision: string,
    voiceId: string,
    pixelRatio: number,
  ): Promise<AccountData<SceneFmPaletteRead>>;
  sceneFmPaletteSample(
    epoch: string,
    receipt: string,
    width: number,
    height: number,
    rgba: number[],
  ): Promise<AccountData<{ background: SceneFmBackground }>>;
  sceneFmPaletteCancel(epoch: string, receipt: string): Promise<void>;
}
export const SCENE_FM_THEME_FALLBACK = 'var(--colorBlack8, var(--scene-fm-theme-fallback))';

// Native Rust owns header parsing, quantization and HSV. Only fixed CSS syntax
// and numeric channels cross this boundary; a null channel preserves official NaN.
export function sceneFmBackgroundCss(background: SceneFmBackground) {
  if (background.kind !== 'gradient') return SCENE_FM_THEME_FALLBACK;
  const channel = (value: number | null) =>
    typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 255
      ? String(value)
      : 'NaN';
  const rgb = (color: SceneFmColor) =>
    `rgb(${channel(color.r)}, ${channel(color.g)}, ${channel(color.b)})`;
  return `linear-gradient(245deg, ${rgb(background.first)} 0%, ${rgb(background.middle)} 48%, ${rgb(background.first)} 100%)`;
}
