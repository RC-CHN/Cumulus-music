import type { CloudResult, CloudUploadInput } from './cloud';
export interface CloudUploadRow {
  requestId: string;
  input: CloudUploadInput;
  error: string;
}
export function uploadRequestId() {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
}
export function uploadBasename(path: string | null) {
  return path?.split(/[\\/]/).pop() ?? '未选择';
}
/** A complete registration reply covers every submitted path exactly once. */
export function validUploadReceipt(result: CloudResult, rows: CloudUploadRow[], epoch: string) {
  const paths = new Set(rows.map((r) => r.input.path)),
    rejected = new Set(result.failed.map((f) => f.id)),
    notStarted = result.notStarted ?? [];
  return (
    result.epoch === epoch &&
    paths.size === rows.length &&
    result.accepted.length + result.failed.length === rows.length &&
    new Set(result.accepted).size === result.accepted.length &&
    result.accepted.every((id) => /^[a-f0-9]{32}$/.test(id)) &&
    rejected.size === result.failed.length &&
    [...rejected].every((path) => paths.has(path)) &&
    notStarted.length <= result.accepted.length &&
    new Set(notStarted.map((f) => f.id)).size === notStarted.length &&
    notStarted.every((f) => paths.has(f.id) && !rejected.has(f.id))
  );
}
