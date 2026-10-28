import { shallowRef } from 'vue';
const active = new Set<Element>();
export const modalDepth = shallowRef(0);
export function modalEntered(element: Element) {
  active.add(element);
  modalDepth.value = active.size;
}
export function modalLeft(element: Element) {
  active.delete(element);
  modalDepth.value = active.size;
}
