export function vinylLyricSelection(root: HTMLElement, selection: Selection | null) {
  if (!selection || selection.isCollapsed || selection.rangeCount !== 1) return;
  const range = selection.getRangeAt(0);
  if (root.contains(range.startContainer) && root.contains(range.endContainer)) return range;
}

export function vinylLyricText(root: HTMLElement, selection: Selection | null) {
  const range = vinylLyricSelection(root, selection);
  if (!range) return;
  const fragments: string[] = [];
  for (const element of root.querySelectorAll('[data-lyric-text]')) {
    if (!range.intersectsNode(element)) continue;
    const text = root.ownerDocument.createRange();
    text.selectNodeContents(element);
    if (range.compareBoundaryPoints(Range.START_TO_START, text) > 0)
      text.setStart(range.startContainer, range.startOffset);
    if (range.compareBoundaryPoints(Range.END_TO_END, text) < 0)
      text.setEnd(range.endContainer, range.endOffset);
    const fragment = text.toString();
    if (fragment) fragments.push(fragment);
  }
  return fragments.join('\n').replace(/[\r\n]+/g, '\n');
}
