const assert=require('node:assert/strict');
const fs=require('node:fs'),vm=require('node:vm'),test=require('node:test');
const script=fs.readFileSync('desktop/src/watchman/compatibility.js','utf8');
function fixture(){
 const actual={userAgent:'Actual engine',platform:'Linux x86_64',languages:['C'],oscpu:'Linux',maxTouchPoints:0};
 Object.defineProperty(actual,'hardwareConcurrency',{get(){assert.equal(this,actual);return 8;}});
 actual.javaEnabled=function(){assert.equal(this,actual);return false;};
 const context=vm.createContext({window:{navigator:actual}});
 const create=vm.runInContext('('+script+')',context);
 return {actual,create,context};
}
const declaration=()=>({userAgent:'Explicit PC declaration',appVersion:'Explicit app version',platform:'Win32',language:'zh-CN',languages:['zh-CN']});
test('scoped projection preserves branded getters/methods and native facts without modifying globals',()=>{
 const {actual,create,context}=fixture();
 const before=vm.runInContext('eval.toString()',context);
 const projected=create(declaration()).navigator;
 assert.equal(projected.platform,'Win32');assert.equal(actual.platform,'Linux x86_64');
 assert.equal(projected.hardwareConcurrency,8);assert.equal(projected.javaEnabled(),false);
 assert.equal(projected.maxTouchPoints,0);assert.equal(projected.oscpu,undefined);assert.equal(actual.oscpu,'Linux');
 assert.equal(before,vm.runInContext('eval.toString()',context));
 assert.equal(Reflect.set(projected,'platform','broken'),false);
 assert.equal(Reflect.defineProperty(projected,'maxTouchPoints',{value:10}),false);
 assert.equal(Reflect.deleteProperty(projected,'languages'),false);
 assert.equal(actual.maxTouchPoints,0);assert.deepEqual(actual.languages,['C']);
});
test('contexts retain their own immutable declarations across source and sibling changes',()=>{
 const {create,actual}=fixture(),source=declaration();
 const first=create(source);source.languages.push('en-US');source.platform='invalid';
 const second=create({...declaration(),language:'en-US',languages:['en-US']});
 assert.equal(first.navigator.platform,'Win32');assert.equal(first.navigator.languages.length,1);
 assert.equal(first.navigator.language,'zh-CN');assert.equal(second.navigator.language,'en-US');
 assert.ok(Object.isFrozen(first));assert.ok(Object.isFrozen(first.navigator.languages));
 assert.deepEqual(actual.languages,['C']);
});
