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

export const commentEmoji = labels.map((label, index) => ({
  label,
  preview: previews[index]!,
}));
const glyphs = new Map(commentEmoji.map(({ label, preview }) => [label, preview]));

export type CommentTextPart =
  | { kind: 'text'; text: string }
  | { kind: 'emoji'; label: string; preview: string };

export function commentTextParts(text: string): CommentTextPart[] {
  const parts: CommentTextPart[] = [];
  let start = 0;
  for (const match of text.matchAll(/\[([^\[\]\r\n]{1,4})\]/g)) {
    const label = match[1]!;
    const preview = glyphs.get(label);
    if (!preview) continue;
    if (match.index > start) parts.push({ kind: 'text', text: text.slice(start, match.index) });
    parts.push({ kind: 'emoji', label, preview });
    start = match.index + match[0].length;
  }
  if (start < text.length) parts.push({ kind: 'text', text: text.slice(start) });
  return parts;
}
