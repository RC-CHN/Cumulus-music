// Reproduce the synthetic H5 link-import vector without loading the H5 or using a network.
import { createCipheriv } from 'node:crypto';

const name = '导入音乐 2026/11/4 12:34:56';
const qq = 'https://y.qq.com/a/%E4%B8%AD?text=专辑#试听';
const kuwo = 'https://www.kuwo.cn/歌单/[42]?ref=A+B';
const inputs = [`分享链接 ${qq}`, '', kuwo, qq];
const csrf = 'fixture+csrf';
const payload = {
  playlistName: name,
  taskIdForLog: '',
  importStarPlaylist: false,
  rqRefer: '',
  songs: JSON.stringify([qq, kuwo, qq].map((url) => ({ name, type: '', url: encodeURI(url) }))),
  csrf_token: csrf,
};
const key = '0123456789ABCDEF';
const modulus = BigInt(
  '0xe0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7',
);
let base = BigInt(`0x${Buffer.from([...key].reverse().join('')).toString('hex')}`);
let exponent = 65537n;
let encrypted = 1n;
while (exponent > 0n) {
  if (exponent & 1n) encrypted = (encrypted * base) % modulus;
  base = (base * base) % modulus;
  exponent >>= 1n;
}
const cbc = (bytes, secret) => {
  const cipher = createCipheriv('aes-128-cbc', secret, '0102030405060708');
  return Buffer.concat([cipher.update(bytes), cipher.final()]).toString('base64');
};
console.log(
  JSON.stringify(
    {
      inputs,
      name,
      csrf,
      key,
      payload,
      params: cbc(cbc(JSON.stringify(payload), '0CoJUm6Qyw8W8jud'), key),
      encSecKey: encrypted.toString(16).padStart(256, '0'),
    },
    null,
    2,
  ),
);
