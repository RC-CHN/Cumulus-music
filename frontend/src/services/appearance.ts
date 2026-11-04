export type BuiltinAppearance = 'system' | 'light' | 'dark' | 'red';
export type AppearancePreference = BuiltinAppearance | 'custom';
export type AppearanceSelection = { theme: BuiltinAppearance } | { theme: 'custom'; color: string };

export const appearanceKey = 'cumulus.appearance.v1';
export const defaultAccent = '#cc2948';

export function customAccent(value: unknown): string | undefined {
  return typeof value === 'string' && value.length === 7 && /^#[0-9a-f]{6}$/i.test(value)
    ? value.toLowerCase()
    : undefined;
}

export function readAppearance(value: unknown): AppearanceSelection {
  if (value === 'light' || value === 'dark' || value === 'red') return { theme: value };
  if (typeof value === 'string' && value.startsWith('{') && value.length <= 96) {
    try {
      const entry = JSON.parse(value) as { theme?: unknown; color?: unknown } | null;
      const color = customAccent(entry?.color);
      if (entry?.theme === 'custom' && color) return { theme: 'custom', color };
    } catch {
      // Fall back to the existing system preference for invalid saved data.
    }
  }
  return { theme: 'system' };
}

export function storeAppearance(value: AppearanceSelection) {
  return value.theme === 'custom' ? JSON.stringify(value) : value.theme;
}

export function resolvedAppearance(preference: AppearancePreference, systemDark: boolean) {
  return preference === 'system' ? (systemDark ? 'dark' : 'light') : preference;
}

// The official custom controls edit hue and value at a fixed saturation of 0.9.
export function sliderAccent(hue: number, brightness: number) {
  const h = (((hue % 360) + 360) % 360) / 60;
  const v = Math.max(0, Math.min(100, brightness)) / 100;
  const chroma = v * 0.9;
  const x = chroma * (1 - Math.abs((h % 2) - 1));
  const colors = [
    [chroma, x, 0],
    [x, chroma, 0],
    [0, chroma, x],
    [0, x, chroma],
    [x, 0, chroma],
    [chroma, 0, x],
  ];
  return (
    '#' +
    colors[Math.floor(h)]!.map((c) =>
      Math.round((c + v - chroma) * 255 + Number.EPSILON * 255)
        .toString(16)
        .padStart(2, '0'),
    ).join('')
  );
}

export function accentSliders(color: string) {
  const rgb = (customAccent(color) ?? defaultAccent).slice(1);
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(rgb.slice(i, i + 2), 16)) as [
    number,
    number,
    number,
  ];
  const max = Math.max(r, g, b);
  const delta = max - Math.min(r, g, b);
  const sector =
    delta === 0
      ? 0
      : max === r
        ? (g - b) / delta
        : max === g
          ? 2 + (b - r) / delta
          : 4 + (r - g) / delta;
  return { hue: Math.round((sector * 60 + 360) % 360), brightness: Math.round(max / 2.55) };
}
