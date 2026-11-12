'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {World}=require('../server/world'),{MinecraftVerifier}=require('../server/minecraft-verify');
test('custom item input uses native data fixing, preserves components and rejects invalid inputs atomically',{skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
 const root=path.resolve('data/verify-calibration/custom-item-supply'),v=new MinecraftVerifier({root,physicsGuard:false,reportEncoding:'rle-v1'}),w=new World();w.set(0,0,0,'minecraft:barrel');w.finalize();
 const put=(tick,slot,item,snbt,data_version=4556)=>({tick,type:'insert',at:[0,0,0],slot,item:'minecraft:'+item,count:1,stack:{data_version,snbt}});
 const actions=[
  put(1,0,'potion','{id:"minecraft:potion",Count:1b,tag:{Potion:"minecraft:strong_healing"}}',3465),
  put(2,1,'enchanted_book','{id:"minecraft:enchanted_book",Count:1b,tag:{StoredEnchantments:[{id:"minecraft:sharpness",lvl:5s}]}}',3465),
  put(3,2,'diamond','{id:"minecraft:diamond",count:1,components:{"minecraft:custom_name":"Token A"}}'),
  put(4,2,'diamond','{id:"minecraft:diamond",count:1,components:{"minecraft:custom_name":"Token B"}}'),
  put(5,3,'diamond','{id:"minecraft:emerald",count:1}'),
  put(6,3,'diamond','{id:"minecraft:diamond",count:2}'),
  put(7,3,'potion','{id:"minecraft:potion",count:1,components:{"minecraft:potion_contents":{potion:"minecraft:not_a_potion"}}}'),
  put(8,3,'diamond','{id:"minecraft:diamond",count:1}',999999),
  {tick:9,type:'drop',at:[.5,3,.5],velocity:[0,0,0],item:'minecraft:potion',count:1,stack:{data_version:4556,snbt:'{id:"minecraft:potion",count:1,components:{"minecraft:potion_contents":{potion:"minecraft:water"}}}'}}
 ];
 const req={probes:[[0,0,0]],inventory_probes:[[0,0,0]],settle_ticks:1,item_entities:true,samples:Array.from({length:11},(_,i)=>i),actions};
 try{
  const run=async(name)=>{const r=await v.run(w,req,{reportPath:path.join(root,name+'.json')});return JSON.parse(fs.readFileSync(r.reportFile));};
  const a=await run('sprint');v.sprint=false;const b=await run('stock');
  for(const r of [a,b]){
   assert.deepEqual(r.inputs.map(i=>i.applied),[true,true,true,false,false,false,false,false,true]);
   const final=r.observations.at(-1),inv=final.blocks[0].inventory;
   assert.equal(inv.length,3);assert.match(inv[0].snbt,/strong_healing/);assert.match(inv[1].snbt,/sharpness.*5/);assert.match(inv[2].snbt,/Token A/);assert.equal(inv[2].count,1);
   assert.equal(final.item_entities.length,1);assert.match(final.item_entities[0].snbt,/minecraft:water/);
  }
  const expand=r=>r.observations.flatMap(o=>Array.from({length:(o.endTick??o.tick)-o.tick+1},(_,i)=>({tick:o.tick+i,blocks:o.blocks,items:o.item_entities.map(({uuid,...e})=>e)})));
  assert.deepEqual(expand(a),expand(b));
 }finally{await v.stop();}
});
