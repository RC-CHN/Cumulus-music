<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { failure } from '../../services/contracts';
import { useCore } from '../../services/core';
import type { ListenedBooksPage, ListenedBooksQuery } from '../../services/history';

const props = defineProps<{ epoch: string; playable: boolean }>();
const emit = defineEmits<{ open: [id: string]; play: [revision: string, key: number] }>();
const core = useCore();
const page = shallowRef<ListenedBooksPage>();
const busy = ref(false);
const error = ref('');
const scroll = ref<HTMLElement>();
let request = 0;
let retry: ListenedBooksQuery = { kind: 'open' };
async function load(query: ListenedBooksQuery) {
  const serial = ++request;
  const owner = props.epoch;
  retry = query;
  busy.value = true;
  error.value = '';
  try {
    const reply = await core.listenedBooks(owner, query);
    if (serial === request && owner === props.epoch && reply.epoch === owner) {
      page.value = reply.value;
      if (scroll.value) scroll.value.scrollTop = 0;
    }
  } catch (cause) {
    if (serial === request && owner === props.epoch) error.value = failure(cause).message;
  } finally {
    if (serial === request) busy.value = false;
  }
}
function turn(number: number) {
  const current = page.value;
  if (!current || busy.value || number < 1 || number > current.pages) return;
  void load({ kind: 'page', revision: current.revision, number });
}
function play(key: number) {
  if (!page.value || !props.playable || busy.value) return;
  emit('play', page.value.revision, key);
}
const counts = new Intl.NumberFormat('zh-CN', { notation: 'compact' });
onMounted(() => void load({ kind: 'open' }));
onBeforeUnmount(() => ++request);
</script>

<template>
  <section class="listened-books" aria-labelledby="listened-books-title" :aria-busy="busy">
    <h1 id="listened-books-title">听过的书</h1>
    <div v-if="error" class="catalog-error" role="alert">
      {{ error }}
      <button class="text-button" :disabled="busy" @click="load(retry)">重新读取</button>
    </div>
    <p v-if="busy && !page" class="muted" role="status">正在读取听书记录…</p>
    <p v-else-if="page && !page.total" class="empty-state">还没有听过的书</p>
    <div v-if="page?.total" ref="scroll" class="listened-books-scroll">
      <table aria-label="听过的书">
        <thead>
          <tr>
            <th scope="col">有声书</th>
            <th scope="col">收藏量</th>
            <th scope="col">声音数</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in page.rows" :key="row.key" :data-listened-book="row.key">
            <td>
              <div class="listened-book">
                <img
                  v-if="row.radio.coverUrl"
                  :src="row.radio.coverUrl"
                  alt=""
                  width="48"
                  height="48"
                  loading="lazy"
                />
                <div class="listened-book-copy">
                  <button
                    class="text-button listened-book-title"
                    :title="row.radio.name"
                    @click="emit('open', row.radio.id)"
                  >
                    {{ row.radio.name }}
                  </button>
                  <div v-if="row.chapter" class="muted listened-book-chapter">
                    <button
                      v-if="row.chapter.playable"
                      class="text-button"
                      :title="row.chapter.name"
                      :disabled="!playable || busy"
                      data-listened-chapter
                      @click="play(row.key)"
                    >
                      {{ row.chapter.name }}
                    </button>
                    <span v-else :title="row.chapter.name">{{ row.chapter.name }}</span>
                    <span v-if="row.chapter.completed">已播完</span>
                    <span v-else-if="row.chapter.percent !== null"
                      >已播 {{ row.chapter.percent }}%</span
                    >
                  </div>
                </div>
              </div>
            </td>
            <td>{{ row.subscribers === null ? '—' : `${counts.format(row.subscribers)}人` }}</td>
            <td>{{ row.radio.programCount ? `${row.radio.programCount}期` : '—' }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <nav v-if="page && page.pages > 1" class="listened-books-pagination" aria-label="听书记录分页">
      <button class="secondary" :disabled="busy || page.number <= 1" @click="turn(page.number - 1)">
        上一页
      </button>
      <span aria-live="polite">{{ page.number }} / {{ page.pages }}</span>
      <button
        class="secondary"
        :disabled="busy || page.number >= page.pages"
        @click="turn(page.number + 1)"
      >
        下一页
      </button>
    </nav>
  </section>
</template>

<style scoped>
.listened-books {
  display: flex;
  min-height: 0;
  height: 100%;
  padding: 24px clamp(16px, 4vw, 48px);
  flex-direction: column;
}
.listened-books h1 {
  margin: 0 0 16px;
  font-size: 22px;
  line-height: 56px;
}
.listened-books-scroll {
  overflow: auto;
  min-height: 0;
}
.listened-books table {
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
}
.listened-books :is(td, th) {
  text-align: start;
  padding: 8px;
}
.listened-books :is(td, th):not(:first-child) {
  width: 84px;
}
.listened-books tbody tr:nth-child(odd) {
  background: var(--surface-alt);
}
.listened-book {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 48px;
}
.listened-book img {
  object-fit: cover;
  border-radius: 6px;
}
.listened-book-copy {
  min-width: 0;
}
.listened-book-title {
  display: block;
  max-width: 100%;
}
.listened-book-chapter {
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 13px;
}
.listened-book-chapter > :last-child {
  flex-shrink: 0;
}
.listened-book-title,
.listened-book-chapter > :first-child {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.listened-book-chapter > :only-child {
  flex-shrink: 1;
}
.listened-books-pagination {
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 12px 0;
  gap: 12px;
}
</style>
