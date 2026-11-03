export type NicknameFormatIssue = 'too_short' | 'too_long' | 'invalid_characters';

// Official M32 counts UTF-16 units above Latin-1 twice, including each surrogate.
export function nicknameLength(value: string): number {
  let length = 0;
  for (let index = 0; index < value.length; index++)
    length += value.charCodeAt(index) <= 255 ? 1 : 2;
  return length;
}

export function nicknameFormatIssue(value: string): NicknameFormatIssue | null {
  const trimmed = value.trim();
  const length = nicknameLength(trimmed);
  if (length < 4) return 'too_short';
  if (length > 30) return 'too_long';
  return /^[\u4e00-\u9fa5A-Za-z0-9_-]*$/.test(trimmed) ? null : 'invalid_characters';
}

export function usableNicknameRecommendation(value: string | null | undefined): value is string {
  return typeof value === 'string' && nicknameFormatIssue(value) === null;
}
