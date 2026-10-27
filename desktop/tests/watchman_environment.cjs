const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),test=require('node:test'),crypto=require('node:crypto');
const manifest=JSON.parse(fs.readFileSync('docs/watchman-environment-source.json'));
function host(entry,mode){
 const timers=[],trace=[];
 const node=()=>({style:{},appendChild(){},removeChild(){},getAttribute(){return null;},offsetWidth:10,offsetHeight:10});
 const document={body:node(),documentElement:{...node(),textContent:'body',innerHTML:'<body>body</body>'},title:'fixture',createElement:node,getElementsByTagName(name){return name==='body'?[this.body]:[];}};
 let audio;
 class OfflineAudio {
  constructor(...args){trace.push(['construct',...args]);if(mode==='constructor_failure')throw Error('fixture');audio=this;this.destination={};}
  createOscillator(){const oscillator={frequency:{value:0},connect(){},disconnect(){trace.push(['oscillator_disconnect']);},start(v){trace.push(['oscillator_start',v,oscillator.type,oscillator.frequency.value]);}};return oscillator;}
  createDynamicsCompressor(){return {threshold:{value:0},knee:{value:0},ratio:{value:0},attack:{value:0},release:{value:0},connect(){},disconnect(){trace.push(['compressor_disconnect']);}};}
  startRendering(){trace.push(['render']);if(mode==='complete')this.oncomplete({renderedBuffer:{getChannelData(index){trace.push(['getChannelData',index]);return new Float32Array(44100);}}});}
 }
 const window={navigator:{},document,history:{length:1},XMLHttpRequest:class{withCredentials=false;},setTimeout(fn,delay){timers.push({fn,delay});},innerWidth:100,innerHeight:90};
 if(mode!=='unsupported')window.OfflineAudioContext=OfflineAudio;
 const context=vm.createContext({window,document,console:{log(){}},trace});
 const script=fs.readFileSync(entry.path,'utf8');assert.equal(crypto.createHash('sha256').update(script).digest('hex'),entry.sha256);
 return {factory:vm.runInContext('('+script+')(undefined)',context),window,trace,timers,get audio(){return audio;}};
}
for(const entry of manifest.measurements){
 test(entry.build+': actual Float32Array type takes the original audio empty-result branch',async()=>{
  const h=host(entry,'complete');const result=await h.factory.audio();
  assert.equal(result.value,'');assert.equal(result.failed,false);
  assert.deepEqual(h.trace,[['construct',1,44100,44100],['oscillator_start',0,'triangle',10000],['render'],['getChannelData',0]]);
  assert.equal(h.timers[0].delay,1000);h.timers[0].fn();
 });
 test(entry.build+': unavailable, throwing and timeout audio finish without invented fingerprint',async()=>{
  for(const mode of ['unsupported','constructor_failure','timeout']){
   const h=host(entry,mode),pending=h.factory.audio();
   if(mode==='timeout'){assert.equal(h.timers.length,1);h.timers[0].fn();h.audio.oncomplete({});}
   const result=await pending;assert.equal(result.value,'');assert.equal(result.failed,false);
  }
 });
 test(entry.build+': missing loader/font dependencies fail explicitly and automation measurements stay real',()=>{
  const h=host(entry,'unsupported'),timing=[1,2,3,4,5,6];
  assert.equal(h.factory.environment('Xb',timing,10).failed,true);
  assert.equal(h.factory.fontCount().failed,true);
  h.window.initWatchman={version:17};assert.equal(h.factory.environment('Xb',timing,10).value,17);
  h.window.navigator.webdriver=true;assert.equal(h.factory.environment('gb',timing,10).value,130);
  assert.equal(h.factory.environment('cc',timing,123).value,123);
  assert.deepEqual(Array.from(h.factory.environment('lb',timing,10).value),[2,3,4,5,6]);
  assert.throws(()=>h.factory.environment('Cookie',timing,10),/unreviewed/);
 });
}
