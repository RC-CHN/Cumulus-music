<script setup lang="ts">
import { onMounted, ref } from 'vue';
import type { MemberWelfareView } from '../../services/memberWelfare';
import Icon from '../Icon.vue';

defineProps<{ view: MemberWelfareView }>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement>();
const close = ref<HTMLButtonElement>();
onMounted(() => {
  dialog.value?.showModal();
  close.value?.focus({ preventScroll: true });
});
</script>

<template>
  <dialog
    ref="dialog"
    class="member-welfare-dialog"
    aria-labelledby="member-welfare-title"
    aria-describedby="member-welfare-description"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 id="member-welfare-title">福利活动中心</h2>
      <button
        ref="close"
        class="icon-button"
        type="button"
        aria-label="关闭福利活动中心"
        @click="emit('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <p id="member-welfare-description">这里只展示活动信息。参与活动请使用网易云音乐官方客户端。</p>
    <ul class="member-welfare-list" aria-label="活动列表" tabindex="0">
      <li v-for="(item, index) in view.items" :key="index">
        <span class="member-welfare-icon" aria-hidden="true"><Icon name="star" /></span>
        <div>
          <h3>
            {{ item.name }}
            <span v-if="item.unread" class="member-welfare-unread" aria-label="未读" role="img" />
          </h3>
          <p v-if="item.description">{{ item.description }}</p>
        </div>
      </li>
    </ul>
  </dialog>
</template>

<style scoped>
.member-welfare-dialog {
  width: min(574px, calc(100vw - 32px));
  max-height: min(562px, calc(100dvh - 40px));
  min-height: min(192px, calc(100dvh - 40px));
  padding: 20px 0 24px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--surface);
  color: var(--text);
  box-shadow: 0 24px 90px #24132833;
  overflow: hidden;
}
.member-welfare-dialog[open] {
  display: flex;
  flex-direction: column;
}
.member-welfare-dialog::backdrop {
  background: var(--overlay);
  backdrop-filter: blur(3px);
}
.member-welfare-dialog header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 24px;
}
.member-welfare-dialog h2,
.member-welfare-dialog h3,
.member-welfare-dialog li p {
  margin: 0;
}
.member-welfare-dialog h2 {
  font-size: 20px;
}
#member-welfare-description {
  color: var(--muted);
  font-size: 13px;
  padding: 0 24px;
  margin: 10px 0 4px;
}
.member-welfare-list {
  list-style: none;
  margin: 0;
  max-height: 474px;
  min-height: 0;
  overflow-y: auto;
  padding: 0;
}
.member-welfare-list li {
  min-height: 74px;
  padding: 8px 40px;
  display: flex;
  gap: 8px;
  align-items: center;
}
.member-welfare-list h3 {
  font-size: 15px;
  overflow-wrap: anywhere;
}
.member-welfare-list p {
  font-size: 13px;
  color: var(--muted);
  overflow-wrap: anywhere;
}
.member-welfare-icon {
  display: grid;
  place-items: center;
  width: 50px;
  height: 50px;
  flex: 0 0 50px;
  border-radius: 6px;
  background: var(--surface-hover);
  color: var(--muted);
}
.member-welfare-unread {
  display: inline-block;
  vertical-align: middle;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  margin-inline-start: 4px;
  background: var(--accent);
}
</style>
