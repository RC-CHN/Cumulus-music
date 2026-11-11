import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  DownloadCommand,
  DownloadDirectoryPreview,
  DownloadsView,
} from '../services/downloads.ts';
import type { DownloadCommandObservation } from './useDownloads.ts';

export interface DownloadDirectorySettingsOwner {
  epoch: string;
  visit: object;
}
interface DirectorySettingsPorts {
  owner(): DownloadDirectorySettingsOwner | undefined;
  blocked(): boolean;
  pick(epoch: string): Promise<string | null>;
  preview(epoch: string, root: string): Promise<DownloadDirectoryPreview>;
  refresh(): Promise<DownloadsView | undefined>;
  commit(
    epoch: string,
    command: Extract<DownloadCommand, { kind: 'change_directory' }>,
  ): Promise<DownloadCommandObservation | undefined>;
  expired(): void;
}
export interface DownloadDirectoryConfirmation {
  owner: DownloadDirectorySettingsOwner;
  preview: DownloadDirectoryPreview;
}

const invalidPreview = '下载目录检查结果无效，请重新选择目录。';
const unknownChange = '尚未确认目录变更结果，请先核对当前设置。';
const recheckFailed = '未能读取当前下载目录，请重新核对。';

const accountExpired = (code: string) => code === 'session_expired' || code === 'profile_changed';

function validReceipt(value: unknown): value is string {
  return typeof value === 'string' && value.length === 32 && /^[0-9a-f]+$/.test(value);
}

function validPreview(preview: DownloadDirectoryPreview): boolean {
  if (
    typeof preview.target !== 'string' ||
    !preview.target ||
    (preview.revision !== null && (typeof preview.revision !== 'string' || !preview.revision))
  )
    return false;
  if (preview.decision === 'ready') return validReceipt(preview.receipt);
  return (
    ['same_directory', 'not_writable', 'insufficient_space'].includes(preview.decision) &&
    preview.receipt === null
  );
}

export function createDownloadDirectorySettings(ports: DirectorySettingsPorts) {
  const activity = ref<'picking' | 'previewing' | 'saving' | 'checking'>();
  const confirmation = shallowRef<DownloadDirectoryConfirmation>();
  const feedback = shallowRef<{ text: string; recheck?: boolean }>();
  let generation = 0;
  function live(owner: DownloadDirectorySettingsOwner) {
    const now = ports.owner();
    return now?.epoch === owner.epoch && now.visit === owner.visit;
  }
  function retire() {
    generation++;
    activity.value = undefined;
    confirmation.value = undefined;
    feedback.value = undefined;
  }
  function dismiss() {
    if (!activity.value) confirmation.value = undefined;
  }
  function reportIssue(cause: unknown, uncertain = false) {
    const issue = failure(cause);
    if (accountExpired(issue.code)) {
      ports.expired();
      return;
    }
    if (uncertain) {
      const detail =
        issue.code === 'stale'
          ? '目录设置已变化或此次选择已失效。'
          : issue.code === 'connection'
            ? ''
            : `${issue.message} `;
      feedback.value = { text: detail + unknownChange, recheck: true };
      return;
    }
    feedback.value = { text: issue.message };
  }
  async function choose(owner = ports.owner()) {
    if (
      !owner ||
      !live(owner) ||
      activity.value ||
      confirmation.value ||
      feedback.value?.recheck ||
      ports.blocked()
    )
      return;
    const ticket = ++generation;
    const current = () => generation === ticket && live(owner);
    feedback.value = undefined;
    activity.value = 'picking';
    try {
      const selected = await ports.pick(owner.epoch);
      if (!selected || !current()) return;
      activity.value = 'previewing';
      const preview = await ports.preview(owner.epoch, selected);
      if (!current()) return;
      if (!preview || preview.epoch !== owner.epoch || !validPreview(preview)) {
        feedback.value = { text: invalidPreview };
        return;
      }
      confirmation.value = { owner, preview: { ...preview } };
    } catch (cause) {
      if (current()) reportIssue(cause);
    } finally {
      if (generation === ticket) activity.value = undefined;
    }
  }
  async function confirm() {
    const shown = confirmation.value;
    if (
      !shown ||
      shown.preview.decision !== 'ready' ||
      !live(shown.owner) ||
      activity.value ||
      ports.blocked()
    )
      return;
    const { owner, preview } = shown;
    const receipt = preview.receipt;
    if (!validReceipt(receipt)) {
      confirmation.value = undefined;
      feedback.value = { text: invalidPreview };
      return;
    }
    const ticket = ++generation;
    const current = () => generation === ticket && live(owner);
    activity.value = 'saving';
    try {
      const observation = await ports.commit(owner.epoch, {
        kind: 'change_directory',
        root: preview.target,
        revision: preview.revision,
        receipt,
      });
      if (!current()) return;
      if (!observation) feedback.value = { text: unknownChange, recheck: true };
      else if ('failure' in observation) {
        // The shared download model already notified account expiry for commands.
        if (!accountExpired(observation.failure.code)) reportIssue(observation.failure, true);
      } else {
        const { result, readback } = observation;
        if (result.epoch !== owner.epoch) {
          feedback.value = { text: unknownChange, recheck: true };
          return;
        }
        if (result.failed.length) {
          feedback.value = { text: result.failed[0]!.message, recheck: true };
          return;
        }
        const accepted = readback?.epoch === owner.epoch ? readback : undefined;
        const configured = accepted?.directory;
        if (
          configured?.root === preview.target &&
          configured.revision &&
          configured.revision !== preview.revision
        )
          feedback.value = { text: '下载目录已更改。' };
        else if (
          result.operationId &&
          accepted?.migration?.id === result.operationId &&
          accepted.migration.target === preview.target &&
          (accepted.migration.phase === 'quiescing' || accepted.migration.phase === 'moving')
        )
          feedback.value = { text: '下载目录迁移已开始。' };
        else feedback.value = { text: '目录变更已提交，请核对当前下载目录。', recheck: true };
      }
    } catch (cause) {
      if (current()) reportIssue(cause, true);
    } finally {
      if (generation === ticket) {
        activity.value = undefined;
        confirmation.value = undefined;
      }
    }
  }
  async function recheck(owner = ports.owner()) {
    if (!owner || !live(owner) || activity.value || confirmation.value) return;
    const ticket = ++generation;
    const current = () => generation === ticket && live(owner);
    activity.value = 'checking';
    try {
      const result = await ports.refresh();
      if (current())
        feedback.value =
          result?.epoch === owner.epoch
            ? { text: '已读取当前下载目录。' }
            : { text: recheckFailed, recheck: true };
    } catch {
      if (current()) feedback.value = { text: recheckFailed, recheck: true };
    } finally {
      if (generation === ticket) activity.value = undefined;
    }
  }
  return { activity, confirmation, feedback, choose, confirm, dismiss, live, recheck, retire };
}

export function directorySettingsDescription(preview: DownloadDirectoryPreview) {
  switch (preview.decision) {
    case 'same_directory':
      return '选择了同样的目录，请重新选择。';
    case 'not_writable':
      return '无法写入该目录，请选择可写的子文件夹。';
    case 'insufficient_space':
      return '该目录存储空间不足，请重新选择目录。';
    case 'ready': {
      const destination = displayDownloadDirectory(preview.target);
      return preview.moveExisting
        ? `将下载目录更改为 ${destination}。转移音乐文件可能花费较长时间，是否继续？`
        : `将下载目录更改为 ${destination}。是否继续？`;
    }
  }
}

// Native canonical Windows paths use the verbatim namespace. Keep that exact
// path in the command; use the familiar drive/share spelling in the UI.
export function displayDownloadDirectory(path: string) {
  const slash = '\\';
  const verbatim = slash.repeat(2) + '?' + slash;
  if (!path.startsWith(verbatim)) return path;
  const plain = path.slice(verbatim.length);
  if (plain.toLowerCase().startsWith('unc' + slash)) return slash.repeat(2) + plain.slice(4);
  if (/^[a-z]:/i.test(plain) && plain[2] === slash) return plain;
  return path;
}
