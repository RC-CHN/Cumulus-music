export function vinylSelection(root: HTMLElement, selection: Selection | null) {
  if (!selection || selection.isCollapsed || selection.rangeCount !== 1) return;
  const range = selection.getRangeAt(0);
  if (root.contains(range.startContainer) && root.contains(range.endContainer)) return range;
}

const copyBlocks = new Set(['P', 'DIV', 'H1', 'H2', 'H3', 'UL', 'LI', 'SECTION']);
function copyChildren(node: Node) {
  return Array.from(node.childNodes, copyNode).join('');
}
function copyNode(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? '';
  if (!(node instanceof Element)) return '';
  if (node.hasAttribute('data-non-selectable')) {
    return node.previousSibling?.nodeType === Node.TEXT_NODE &&
      node.nextSibling?.nodeType === Node.TEXT_NODE
      ? '\n'
      : '';
  }
  if (node.tagName === 'BR') return '\n';
  return copyChildren(node) + (copyBlocks.has(node.tagName) ? '\n' : '');
}

export function vinylCopyText(root: HTMLElement, selection: Selection | null) {
  const range = vinylSelection(root, selection);
  if (!range) return;
  const fragments: string[] = [];
  for (const element of root.querySelectorAll('[data-vinyl-copy-text], [data-lyric-text]')) {
    if (
      !range.intersectsNode(element) ||
      element.closest('[data-non-selectable], [inert], [hidden]') ||
      !element.getClientRects().length
    )
      continue;
    const text = root.ownerDocument.createRange();
    text.selectNodeContents(element);
    if (range.compareBoundaryPoints(Range.START_TO_START, text) > 0)
      text.setStart(range.startContainer, range.startOffset);
    if (range.compareBoundaryPoints(Range.END_TO_END, text) < 0)
      text.setEnd(range.endContainer, range.endOffset);
    const fragment = copyChildren(text.cloneContents());
    if (fragment) fragments.push(fragment);
  }
  // The official clipboard serializer includes the outer block's final LF.
  if (fragments.length) return `${fragments.join('\n')}\n`.replace(/[\r\n]+/g, '\n');
}
