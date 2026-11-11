import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  DownloadCommand,
  DownloadDirectoryPreview,
  DownloadResult,
} from '../services/downloads.ts';

export interface DownloadDirectorySettingsOwner {
  epoch: string;
  visit: object;
}
interface DirectorySettingsPorts {
  owner(): DownloadDirectorySettingsOwner | undefined;
  blocked(): boolean;
  pick(epoch: string): Promise<string | null>;
  preview(epoch: string, root: string): Promise<DownloadDirectoryPreview>;
  commit(
    epoch: string,
    command: Extract<DownloadCommand, { kind: 'change_directory' }>,
  ): Promise<DownloadResult | undefined>;
  expired(): void;
}
export interface DownloadDirectoryConfirmation {
  owner: DownloadDirectorySettingsOwner;
  preview: DownloadDirectoryPreview;
}

export function createDownloadDirectorySettings(ports: DirectorySettingsPorts) {
  const activity = ref<'picking' | 'previewing' | 'saving'>();
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
    if (['session_expired', 'profile_changed'].includes(issue.code)) {
      ports.expired();
      return;
    }
    if (uncertain) {
      feedback.value = { text: '尚未确认目录变更结果，请先核对当前设置。', recheck: true };
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
      if (!current() || preview.epoch !== owner.epoch || !preview.target) return;
      confirmation.value = { owner, preview };
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
    const ticket = ++generation;
    const current = () => generation === ticket && live(owner);
    activity.value = 'saving';
    try {
      const result = await ports.commit(owner.epoch, {
        kind: 'change_directory',
        root: preview.target,
        revision: preview.revision,
      });
      if (!current()) return;
      if (!result || result.epoch !== owner.epoch)
        feedback.value = { text: '尚未确认目录变更结果，请先核对当前设置。', recheck: true };
      else if (result.failed.length)
        feedback.value = { text: result.failed[0]!.message, recheck: true };
      else
        feedback.value = {
          text: result.operationId ? '下载目录迁移已开始。' : '下载目录已更改。',
        };
    } catch (cause) {
      if (current()) reportIssue(cause, true);
    } finally {
      if (generation === ticket) {
        activity.value = undefined;
        confirmation.value = undefined;
      }
    }
  }
  return { activity, confirmation, feedback, choose, confirm, dismiss, live, retire };
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
