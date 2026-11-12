'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {World}=require('../server/world'),{MinecraftVerifier}=require('../server/minecraft-verify');
test('native broken shulker distinguishes absent from empty and preserves dropped contents without changing block timing',{skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
 const root=path.resolve('data/verify-calibration/dynamic-inventory'),v=new MinecraftVerifier({root,physicsGuard:false,reportEncoding:'rle-v1'}),w=new World();
 w.set(0,0,0,'minecraft:stone');w.set(0,1,0,'minecraft:piston',{facing:'east',extended:'false'});w.set(-1,1,0,'minecraft:lever',{face:'wall',facing:'west',powered:'false'});w.set(1,0,0,'minecraft:stone');w.set(1,1,0,'minecraft:shulker_box',{facing:'up'});w.set(4,0,0,'minecraft:barrel',{facing:'up',open:'false'});w.finalize();
 const base={probes:[[0,1,0],[1,1,0],[4,0,0]],samples:Array.from({length:61},(_,i)=>i),settle_ticks:1,actions:[{tick:10,type:'toggle',at:[-1,1,0]}],block_entities:[{at:[1,1,0],data_version:4556,snbt:'{id:"minecraft:shulker_box",Items:[{Slot:0b,id:"minecraft:stone",count:7}]}'}]};
 const expand=r=>r.observations.flatMap(o=>Array.from({length:(o.endTick??o.tick)-o.tick+1},(_,i)=>({...o,tick:o.tick+i})));
 try{
  const run=async(name,extra)=>{const result=await v.run(w,{...base,...extra},{reportPath:path.join(root,name+'.json')});return JSON.parse(fs.readFileSync(result.reportFile));};
  const stock=await run('baseline',{}),observed=await run('observed',{inventory_probes:[[1,1,0],[4,0,0]],inventory_missing:'report',item_entities:true});
  const a=expand(stock),b=expand(observed);
  assert.deepEqual(b.map(o=>o.blocks.map(({block,signal})=>({block,signal}))),a.map(o=>o.blocks),'observation must not change native block transitions');
  assert.ok(b.some(o=>o.blocks[1].inventory===null),'destroyed container must report explicit null');
  assert.ok(b.every(o=>Array.isArray(o.blocks[2].inventory)&&o.blocks[2].inventory.length===0),'existing empty container is []');
  assert.ok(b.some(o=>o.item_entities.some(e=>e.id==='minecraft:shulker_box'&&/minecraft:stone/.test(e.snbt)&&/count:7/.test(e.snbt))),'dropped box must retain all nested contents');
  assert.ok(b.every(o=>Array.isArray(o.item_entities)),'entity samples must be explicit even when empty');
  await assert.rejects(run('strict',{inventory_probes:[[1,1,0]]}),/not a container/);
 }finally{await v.stop();}
});
