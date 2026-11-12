'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {World}=require('../server/world'),{MinecraftVerifier}=require('../server/minecraft-verify');
test('living census observes a native bucket release without changing physics',{skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
 const root=path.resolve('data/verify-calibration/living-census'),v=new MinecraftVerifier({root,physicsGuard:false,reportEncoding:'rle-v1'}),w=new World();
 for(let x=-3;x<=3;x++)for(let z=-4;z<=2;z++)w.set(x,0,z,'minecraft:stone');
 w.set(0,2,0,'minecraft:dispenser',{facing:'north',triggered:'false'});w.set(1,2,0,'minecraft:lever',{face:'wall',facing:'east',powered:'false'});w.finalize();
 const req={probes:[[0,2,0],[0,2,-1],[1,2,0]],inventory_probes:[[0,2,0]],world_inventories:true,item_entities:true,settle_ticks:1,samples:Array.from({length:81},(_,i)=>i),actions:[{tick:1,type:'insert',at:[0,2,0],slot:0,item:'minecraft:axolotl_bucket',count:1,stack:{data_version:4556,snbt:'{id:"minecraft:axolotl_bucket",count:1,components:{"minecraft:bucket_entity_data":{NoAI:1b,Health:14.0f}}}'}},{tick:2,type:'toggle',at:[1,2,0]}]};
 try{
  const run=async(name,living)=>{const r=await v.run(w,{...req,living_entities:living},{reportPath:path.join(root,name+'.json')});return JSON.parse(fs.readFileSync(r.reportFile));};
  const observed=await run('sprint-census',true),silent=await run('sprint-without-census',false);v.sprint=false;const stock=await run('stock-census',true);
  for(const r of [observed,silent,stock])assert(r.inputs.every(i=>i.applied));
  for(const r of [observed,stock]){
   assert.equal(r.observations[0].living_entities.length,0);
   const alive=r.observations.at(-1).living_entities;assert.equal(alive.length,1);assert.equal(alive[0].type,'minecraft:axolotl');assert.equal(alive[0].health,14);assert.equal(alive[0].max_health,14);assert.equal(alive[0].dead,false);assert(alive[0].at.every(Number.isFinite));assert.match(alive[0].uuid,/^[0-9a-f-]{36}$/);
  }
  assert(silent.observations.every(o=>!('living_entities' in o)));
  const expand=(r,withLiving)=>r.observations.flatMap(o=>Array.from({length:(o.endTick??o.tick)-o.tick+1},(_,i)=>({tick:o.tick+i,blocks:o.blocks,item_entities:o.item_entities.map(({uuid,...e})=>e),world_inventories:o.world_inventories.map(({uuid,...e})=>e),...(withLiving?{living_entities:o.living_entities.map(({uuid,...e})=>e)}:{})})));
  assert.deepEqual(expand(observed,false),expand(silent,false));assert.deepEqual(expand(observed,true),expand(stock,true));
 }finally{await v.stop();}
});
