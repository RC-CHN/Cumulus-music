<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from 'vue';

const props = defineProps<{ disabled: boolean }>();
const emit = defineEmits<{ select: [token: string] }>();
// Official TextArea/emojiList.jsx order. These are system glyph previews;
// the comment itself carries the official bracketed label.
const labels =
  '大笑 可爱 憨笑 色 亲亲 惊恐 流泪 亲 呆 哀伤 呲牙 吐舌 撇嘴 怒 奸笑 汗 痛苦 惶恐 生病 口罩 大哭 晕 发怒 开心 鬼脸 皱眉 流感 爱心 心碎 钟情 星星 生气 便便 强 弱 拜 牵手 跳舞 禁止 这边 爱意 示爱 嘴唇 狗 猫 猪 兔子 小鸡 公鸡 幽灵 圣诞 外星 钻石 礼物 男孩 女孩 蛋糕 18 圈 叉'.split(
    ' ',
  );
const previews =
  '😄 😊 😆 😍 😘 😱 😢 😚 😐 😔 😁 😛 😕 😡 😏 😓 😣 😰 🤒 😷 😭 😵 😠 😀 😜 😟 🤧 ❤️ 💔 💘 ⭐ 💢 💩 👍 👎 🙏 🤝 💃 🚫 👈 💕 💌 👄 🐶 🐱 🐷 🐰 🐤 🐓 👻 🎅 👽 💎 🎁 👦 👧 🎂 🔞 ⭕ ❌'.split(
    ' ',
  );
const id = useId();
const open = ref(false);
const page = ref(0);
const trigger = ref<HTMLButtonElement>();
const picker = ref<HTMLElement>();
const entries = computed(() => {
  const start = page.value ? 32 : 0;
  return labels.slice(start, page.value ? undefined : 32).map((label, index) => ({
    label,
    preview: previews[start + index],
  }));
});

watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) open.value = false;
  },
);
async function toggle() {
  if (open.value) return close();
  page.value = 0;
  open.value = true;
  await nextTick();
  picker.value?.querySelector<HTMLButtonElement>('.emoji-option')?.focus({ preventScroll: true });
}
function close(restore = true) {
  open.value = false;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
function leave(event: FocusEvent) {
  if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null))
    close(false);
}
function escape(event: KeyboardEvent) {
  if (!open.value) return;
  event.preventDefault();
  event.stopPropagation();
  close();
}
function choose(label: string) {
  if (props.disabled) return;
  close(false);
  emit('select', `[${label}]`);
}
function showPage(index: number, event: MouseEvent) {
  (event.currentTarget as HTMLButtonElement).focus({ preventScroll: true });
  page.value = index;
}
</script>

<template>
  <div class="comment-emoji" @focusout="leave" @keydown.esc="escape">
    <button
      ref="trigger"
      class="text-button emoji-trigger"
      type="button"
      :disabled="disabled"
      :aria-expanded="open"
      :aria-controls="open ? id : undefined"
      @click="toggle"
    >
      <span aria-hidden="true">☺</span> 表情
    </button>
    <div v-if="open" :id="id" ref="picker" class="emoji-picker" role="group" aria-label="评论表情">
      <header>
        <span>表情</span>
        <button type="button" class="text-button" aria-label="关闭表情" @click="close()">×</button>
      </header>
      <div class="emoji-grid">
        <button
          v-for="{ label, preview } in entries"
          :key="label"
          type="button"
          class="emoji-option"
          :aria-label="label"
          :title="label"
          @click="choose(label)"
        >
          {{ preview }}
        </button>
      </div>
      <footer>
        <button
          v-for="index in 2"
          :key="index"
          type="button"
          class="text-button"
          :aria-pressed="page === index - 1"
          :aria-label="`表情第 ${index} 页`"
          @click="showPage(index - 1, $event)"
        >
          {{ index }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.comment-emoji {
  position: relative;
  margin-right: auto;
}
.emoji-trigger {
  min-height: 32px;
  white-space: nowrap;
}
.emoji-trigger span {
  font-size: 18px;
  vertical-align: middle;
}
.emoji-picker {
  position: absolute;
  z-index: 1;
  top: 100%;
  inset-inline-start: 0;
  width: max-content;
  max-width: min(280px, 65vw);
  padding: 8px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--surface-raised);
  box-shadow: 0 8px 24px #0003;
  color: var(--text);
}
.emoji-picker header,
.emoji-picker footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.emoji-picker footer {
  justify-content: center;
  gap: 8px;
}
.emoji-picker footer [aria-pressed='true'] {
  color: var(--accent);
}
.emoji-grid {
  display: grid;
  grid-template-columns: repeat(8, minmax(0, 1fr));
  gap: 2px;
}
.emoji-option {
  width: 28px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 4px;
  background: transparent;
  font-size: 16px;
  cursor: pointer;
}
.emoji-option:hover {
  background: var(--surface-alt);
}
</style>
