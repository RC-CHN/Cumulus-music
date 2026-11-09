export interface FontFamily {
  name: string;
  aliases: string[];
}
export interface FontCatalog {
  epoch: string;
  families: FontFamily[];
  fallbacks: string[];
}
export interface FontClient {
  read(epoch: string): Promise<FontCatalog>;
}
export interface InterfaceFont {
  family: string | null;
  sizePx: 12 | 13 | 14 | 15;
}
export interface InterfaceFontClient {
  read(): Promise<InterfaceFont>;
  write(family: string | null): Promise<InterfaceFont>;
  catalog(epoch: string): Promise<{ epoch: string; choices: string[] }>;
}

export const interfaceFontPageSize = 128;

export function interfaceFontPage(choices: string[], selected: string | null, page: number) {
  const start = page * interfaceFontPageSize;
  const visible = choices.slice(start, start + interfaceFontPageSize);
  return selected && !visible.includes(selected) ? [selected, ...visible] : visible;
}

export function interfaceFontCss({ family, sizePx }: InterfaceFont) {
  return {
    family: family ? JSON.stringify(family) : '',
    size: `${sizePx}px`,
  };
}
export function fontStack(names: string[]): string {
  return [...new Set(names.filter(Boolean))]
    .map((name) => JSON.stringify(name))
    .concat('system-ui', 'sans-serif')
    .join(', ');
}
