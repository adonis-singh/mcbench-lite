'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {World}=require('../server/world'),{MinecraftVerifier}=require('../server/minecraft-verify');
test('native timed item supply has exact identity and inlet state; stock and sprint trajectories match',{skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
 const root=path.resolve('data/verify-calibration/item-supply'),v=new MinecraftVerifier({root,physicsGuard:false,reportEncoding:'rle-v1'}),w=new World();w.set(0,0,0,'minecraft:stone');w.finalize();
 const request={probes:[[0,0,0]],settle_ticks:1,item_entities:true,samples:Array.from({length:21},(_,i)=>i),actions:[{tick:3,type:'drop',at:[.5,5,.5],velocity:[.1,0,0],item:'minecraft:diamond',count:7}]};
 const expand=r=>r.observations.flatMap(o=>Array.from({length:(o.endTick??o.tick)-o.tick+1},(_,i)=>({...o,tick:o.tick+i})));
 try{
  const run=async(name,req=request)=>{const r=await v.run(w,req,{reportPath:path.join(root,name+'.json')});return JSON.parse(fs.readFileSync(r.reportFile));};
  const fast=await run('sprint');v.sprint=false;const stock=await run('stock');
  const a=expand(fast),b=expand(stock);assert.equal(fast.inputs[0].applied,true);assert.equal(stock.inputs[0].applied,true);
  const first=a.find(o=>o.tick===3).item_entities;assert.equal(first.length,1);assert.equal(first[0].uuid,fast.inputs[0].entityUUID);assert.deepEqual(first[0].at,[.5,5,.5]);assert.equal(first[0].count,7);
  assert.ok(a.filter(o=>o.tick<3).every(o=>!o.item_entities.length));
  assert.deepEqual(a.map(o=>o.item_entities.map(({uuid,...e})=>e)),b.map(o=>o.item_entities.map(({uuid,...e})=>e)));
  const invalid=await run('oversized',{...request,actions:[{...request.actions[0],item:'minecraft:ender_pearl',count:17}]});
  assert.equal(invalid.inputs[0].applied,false);assert.ok(invalid.observations.every(o=>!o.item_entities.length));
 }finally{await v.stop();}
});
