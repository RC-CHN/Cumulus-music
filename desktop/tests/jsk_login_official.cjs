// Source oracle; private recovery inputs are deliberately absent from public CI.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const parser = require('../../frontend/node_modules/@babel/parser');
const root = path.resolve(process.argv[2] || 'references/netease-3.1.42.205594-analysis/recovered-js/modules');
const names = ['hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js', 'hybrid_app.chunk.1482700000000.js/290.js'];
const sources = names.map(name => fs.readFileSync(path.join(root, name), 'utf8'));
function walk(node, visit) {
  if (!node || typeof node !== 'object') return;
  visit(node);
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach(child => walk(child, visit));
    else if (value && typeof value === 'object') walk(value, visit);
  }
}
const ast = sources.map(source => parser.parse(source));
function method(name, contains = '') {
  const found = [];
  walk(ast[0], node => {
    if (node.type !== 'ObjectExpression' || !node.properties.some(p => p.key?.name === 'key' && p.value?.value === name)) return;
    const fn = node.properties.find(p => p.key?.name === 'value')?.value;
    if (fn && sources[0].slice(fn.start, fn.end).includes(contains)) found.push(fn);
  });
  assert.equal(found.length, 1, 'unique official method: ' + name);
  return found[0];
}
const code = node => sources[0].slice(node.start, node.end);
const digest = text => ({ toString: () => crypto.createHash('md5').update(text).digest('hex') });
const encrypted = vm.runInNewContext('(' + code(method('encryptPassword')) + ')', { m: { default: { MD5: digest } } });
const ids = ['01234567-89ab-4cde-8f01-23456789abcd', 'fedcba98-7654-4321-8765-fedcba987654'];
let index = 0;
const builder = vm.runInNewContext('(' + code(method('buildLoginParams', 'countrycode:')) + ')');
const phone = builder.call({ securityManager: { generateUUID: () => ids[index++] } }, {
  phone: '13800138000', countryCode: '86', password: encrypted('fixture-password'),
});
assert.equal(index, 2);
let appendContext, appendHeaders, qrCreate, qrPoll, email;
const fetch = method('fetch', 'x-login-chain-id');
walk(fetch, node => {
  if (node.type === 'AssignmentExpression' && node.left.name === 'a' && code(node.right).includes('fromPage:')) appendContext = node.right;
  if (node.type === 'AssignmentExpression' && node.left.property?.name === 'header' && code(node.right).includes('x-music-tu')) appendHeaders = node.right;
});
walk(method('getQrcodeUnikey'), node => {
  if (node.type === 'ObjectExpression' && code(node).includes('type: e.clientType || 5')) qrCreate = node;
});
walk(method('checkQrcodeStatus'), node => {
  if (node.type === 'ObjectExpression' && node.properties.some(p => p.key?.name === 'key') && code(node).includes('this.currentUnikey')) qrPoll = node;
});
walk(method('neteaseEmailLogin'), node => {
  if (node.type === 'CallExpression' && node.callee.property?.name === 'fetch' && node.arguments[0]?.value === '/api/login') email = node.arguments[1];
});
assert(appendContext && appendHeaders && qrCreate && qrPoll && email);
const context = payload => vm.runInNewContext('(' + code(appendContext) + ')', { U: Object.assign, n: payload });
const headers = ticket => vm.runInNewContext('(' + code(appendHeaders) + ')', { U: Object.assign, We: () => '', u: ticket });
const smsFunction = method('sendLoginRequestWithUrsToken');
let smsBase;
const smsAssignments = [];
walk(smsFunction, node => {
  if (node.type === 'AssignmentExpression' && node.left.name === 'a' && node.right.type === 'ObjectExpression' && node.right.properties.some(p => p.key?.name === 'captcha')) smsBase = node;
  if (node.type === 'LogicalExpression' && node.right?.type === 'AssignmentExpression' && node.right.left.object?.name === 'a' && ['ursToken', 'countryCode'].includes(node.right.left.property?.name)) smsAssignments.push(node);
});
assert(smsBase && smsAssignments.length === 2);
const sms = vm.runInNewContext('(()=>{let a;' + code(smsBase) + ';' + smsAssignments.map(code).join(';') + ';return a})()', { n:'13800138000', r:'123456', i:'86', e:null });
const mail = vm.runInNewContext('(function(){return (' + code(email) + ')}).call(receiver)', { U:Object.assign, e:{username:'me@example.test',password:'fixture-password'}, receiver:{securityManager:{encryptPassword:encrypted}} });
const qr = expr => vm.runInNewContext('(function(){return (' + code(expr) + ')}).call(receiver)', { e:{}, receiver:{currentUnikey:'fixture-qr-key'} });
let bridge;
walk(ast[1], node => { if (node.type === 'ObjectProperty' && node.key?.name === 'verifySms') bridge = sources[1].slice(node.value.start,node.value.end); });
assert(bridge);
(async () => {
  let result;
  await vm.runInNewContext('('+bridge+')')({}, value => {result=value});
  assert.equal(result.result.success,true);assert.equal(result.result.ursToken,'');
  const value = { sources:Object.fromEntries(names.map((name,i)=>[name,crypto.createHash('sha256').update(sources[i]).digest('hex')])),
    phonePassword:context(phone),phoneSms:context(sms),email:context(mail),qrCreate:context(qr(qrCreate)),qrPoll:context(qr(qrPoll)),headers:headers(''),challengeHeaders:headers('fixture-ticket'),pcVerifySms:result };
  console.log(JSON.stringify(value,null,2));
})().catch(error => {console.error(error);process.exitCode=1});
