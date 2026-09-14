import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import vm from 'node:vm'
const template=readFileSync(process.env.LOADER_TEMPLATE || new URL('../src/templates/split-loader.js',import.meta.url),'utf8')
function sandbox(decoder) {
 const context=vm.createContext({module:{exports:{}},TextDecoder:decoder})
 const source=template.replaceAll('__DCL_ONE_SDK_CHUNK__','sdk.js').replaceAll('__DCL_ONE_SCENE_CHUNK__','scene.js').replaceAll('__DCL_ONE_SMART_CHUNK__','').replaceAll('__DCL_ONE_MAX_COMPOSITE_ENTITY__','0').replaceAll('__DCL_ONE_MP__','false')
 vm.runInContext(source,context)
 return context
}
const sample='Constitución · Campaña · Paraná · ¿Por qué? ¡ñ! 日本語 🧉'
for (const [name,decoder] of [['absent',undefined],['throws',class {decode(){throw Error('unavailable')}}],['native',TextDecoder]]) {
 test(`UTF-8 with TextDecoder ${name}`,()=>{
  const c=sandbox(decoder)
  for(const text of ['',sample,'a'.repeat(32767)+sample,'\ufeff'+sample,sample+'\ufeff']) {
   const bytes=Buffer.from(text)
   assert.equal(c.__dclOneDecode(bytes),new TextDecoder().decode(bytes))
  }
 })
}
test('fallback matches replacement semantics for malformed bytes',()=>{
 const c=sandbox(undefined),native=new TextDecoder()
 const cases=[[0xc0,0xaf],[0xed,0xa0,0x80],[0xf4,0x90,0x80,0x80],[0xe2,0x82],[0xe2,0x82,0x41],[0xef,0xbb,0xbf,0xef,0xbb,0xbf],[0xff,0xfe]]
 // Deterministic fuzzing includes isolated continuation bytes and truncated sequences.
 let seed=42
 for(let i=0;i<1000;i++) {const bytes=[];for(let j=0;j<i%32;j++){seed=(Math.imul(seed,1664525)+1013904223)>>>0;bytes.push(seed>>>24)}cases.push(bytes)}
 for(const bytes of cases)assert.equal(c.__dclOneDecode(Uint8Array.from(bytes)),native.decode(Uint8Array.from(bytes)),JSON.stringify(bytes))
})
test('actual loader starts UTF-8 SDK and scene chunks without TextDecoder',async()=>{
 const c=sandbox(undefined)
 const files={[c.__dclOneSdkChunkPath]:`module.exports = { greeting: '${sample}' }`,[c.__dclOneSceneChunkPath]:`const sdk=require('greeting'); module.exports.onStart=()=>{globalThis.result=sdk+' / ${sample}'}; module.exports.onUpdate=()=>{}`}
 c.require=(name)=>{assert.equal(name,'~system/Runtime');return {readFile:async({fileName})=>({content:Uint8Array.from(Buffer.from(files[fileName]))})}}
 await c.module.exports.onStart()
 assert.equal(c.result,sample+' / '+sample)
})
