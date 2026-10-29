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
export function fontStack(names: string[]): string {
  return [...new Set(names.filter(Boolean))]
    .map((name) => JSON.stringify(name))
    .concat('system-ui', 'sans-serif')
    .join(', ');
}
