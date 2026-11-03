// Optional offline oracle: execute the complete recovered refreshToken method.
// Synthetic platform I/O replaces the network and native store, not the method.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const parser = require('../../frontend/node_modules/@babel/parser');
const path = 'references/netease-3.1.42.205594-analysis/recovered-js/modules/hybrid_vendors_app_subApp.chunk.1482700000000.js/1368.js';
const source = fs.readFileSync(path, 'utf8');
const tree = parser.parse(source);
function walk(node, visit, parent) {
  if (!node || typeof node !== 'object') return;
  visit(node, parent);
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach(child => walk(child, visit, node));
    else if (value && typeof value === 'object') walk(value, visit, node);
  }
}
let block;
const methods = {};
const wanted = ['refreshToken', 'encryptParams', 'getTokenRefreshTime', 'getLoginInfo', 'updateTokenInfo', 'updateLoginInfoToStorage', 'subscribeForegroundChange'];
walk(tree, (node, parent) => {
  if (node.type === 'FunctionDeclaration' && node.id.name === 'O') block = parent;
  if (node.type === 'ObjectExpression') {
    const name = node.properties.find(p => p.key?.name === 'key')?.value?.value;
    if (wanted.includes(name)) {
      const fn = node.properties.find(p => p.key?.name === 'value')?.value;
      if (fn && (name !== 'getLoginInfo' || source.slice(fn.start, fn.end).includes('tokenInfo: i'))) {
        assert(!methods[name], `ambiguous method ${name}`);
        methods[name] = source.slice(fn.start, fn.end);
      }
    }
  }
});
assert(block);
for (const name of wanted) assert(methods[name], name);
const helpers = block.body.filter(n => n.type === 'FunctionDeclaration').map(n => source.slice(n.start, n.end)).join('\n');
const now = 1730000000000;
const base = {enabled:true, lastMs:now - 86400000, nowMs:now, musicU:'synthetic-old-u', refreshToken:'synthetic-old-rt', device:{deviceName:'Desk,Model,2', deviceDisplayName:'Fallback', deviceId:'synthetic-device', os:'os-fallback'}, env:{clientType:'pc'}, ydToken:'', code:200, newMusicU:'synthetic-new-u', newRefreshToken:'synthetic-new-rt'};
const scenarios = [
  {name:'ab_off', enabled:false},
  {name:'new_login', lastMs:now},
  {name:'before_day', lastMs:now - 86399000},
  {name:'clock_backwards', lastMs:now + 1000},
  {name:'day_boundary'},
  {name:'missing_login_time', lastMs:0},
  {name:'missing_music_u', musicU:''},
  {name:'missing_refresh_token', refreshToken:''},
  {name:'success_without_either_header', newMusicU:'', newRefreshToken:''},
  {name:'refresh_header_alone', newMusicU:''},
  {name:'music_u_alone', newRefreshToken:''},
  {name:'business_rejection', code:500},
  {name:'transport_error', transportError:true},
  {name:'device_name_fallback', device:{...base.device,deviceName:'',deviceDisplayName:'模型 A'}},
  {name:'env_failure_os_fallback', envError:true},
  {name:'device_token_preserved', ydToken:'synthetic-provider-token'},
];
(async () => {
  const cases = [];
  for (const scenario of scenarios) {
    const input = {...base, ...scenario};
    const calls = [], writes = [], updates = [], telemetry = [], events = [];
    let deviceReads = 0, tokenReads = 0, listener;
    const context = {
      Ve:{log(){},error(){}}, Date:{now:()=>input.nowMs},
      Qe:{REFRESH_TOKEN:'refreshToken'}, Ue:{TOKEN_UPDATED:'TOKEN_UPDATED'}, Se:{APP_BECOME_ACTIVE:'APP_BECOME_ACTIVE'},
      m:{enc:{Utf8:{parse:v=>v}}, MD5:v=>crypto.createHash('md5').update(v,'utf8').digest('hex')},
      record:(...v)=>telemetry.push(v),
    };
    vm.createContext(context);
    vm.runInContext(helpers + '\nJe=(...v)=>record(...v);globalThis.methods={' + Object.entries(methods).map(([k,v])=>`${k}:(${v})`).join(',') + '};',context,{timeout:1000});
    const platform = {
      checkAB:async()=>input.enabled,
      getCurrentEnv:async()=>{if(input.envError)throw Error('synthetic env');return input.env;},
      getItem:async()=>String(input.lastMs),
      setItem:(key,value)=>writes.push({key,value}),
      getLoginInfo:async()=>({userInfo:{account:{id:123}},tokenInfo:{MUSIC_U:input.musicU,refreshToken:input.refreshToken}}),
      getDeviceInfo:async()=>{deviceReads++;return input.device;},
      getYdDeviceToken:async()=>{tokenReads++;return input.ydToken;},
      fetch:async(path,payload,method,options)=>{
        calls.push(JSON.parse(JSON.stringify({path,payload,method,options})));
        if(input.transportError)throw Error('synthetic transport');
        return {body:{code:input.code},header:{'Set-Cookie':input.newMusicU?`MUSIC_U=${input.newMusicU}; Path=/; Secure`:'','x-refresh-token':input.newRefreshToken}};
      },
      updateLoginInfo:async(value,type)=>{updates.push(JSON.parse(JSON.stringify({value,type})));},
    };
    const state = {...context.methods,getPlatform:()=>platform,eventEmitter:{on:(name,fn)=>{assert.equal(name,'APP_BECOME_ACTIVE');listener=fn;},emit:event=>events.push(JSON.parse(JSON.stringify(event)))}};
    // Exercise the original foreground subscription as well as its handler.
    state.subscribeForegroundChange();
    assert(listener);
    await state.refreshToken();
    const skip = telemetry.find(v=>v[1] === 'skip')?.[2];
    const outcome = updates.length ? 'refreshed' : skip === 'time_not_reached' ? 'not_due' : skip === 'no_music_u' || skip === 'no_refresh_token' ? 'missing_credentials' : !input.enabled ? 'policy_disabled' : input.transportError || input.code !== 200 ? 'request_failed' : 'missing_pair';
    if (outcome !== 'refreshed') {assert.equal(updates.length,0);assert.equal(writes.length,0);}
    else {
      assert.equal(updates.length,1);
      assert.equal(writes[0].key,'jsk_token_last_refresh_time');
      assert.equal(writes[0].value,String(input.nowMs));
    }
    cases.push({name:input.name,input,expected:{outcome,calls,deviceReads,tokenReads,updates,writes,events}});
  }
  const report = {schema:1,source:path,sha256:crypto.createHash('sha256').update(source).digest('hex'),scope:'Full recovered refreshToken and storage/foreground methods with synthetic platform dependencies; no network or whole SDK bootstrap',cases};
  const output = JSON.stringify(report,null,2)+'\n';
  if(process.argv[2]) fs.writeFileSync(process.argv[2],output); else process.stdout.write(output);
})().catch(error=>{console.error(error);process.exitCode=1;});
