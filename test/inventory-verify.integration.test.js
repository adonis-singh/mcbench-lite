'use strict';
const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
const {World}=require('../server/world');
const {MinecraftVerifier}=require('../server/minecraft-verify');

test('native inventory import preserves old items and components, transfers at identical stock tick timings, and never silently overwrites', {skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
 const root=path.resolve('data/verify-calibration/inventory');
 const verifier=new MinecraftVerifier({root,physicsGuard:false});
 const world=new World();
 world.set(0,2,0,'minecraft:barrel',{facing:'up',open:'false'});
 world.set(0,1,0,'minecraft:hopper',{facing:'down',enabled:'true'});
 world.set(0,0,0,'minecraft:barrel',{facing:'up',open:'false'});
 world.set(4,0,0,'minecraft:barrel',{facing:'up',open:'false'});
 world.set(8,2,0,'minecraft:note_block',{instrument:'harp',note:'0',powered:'false'});
 world.set(8,1,0,'minecraft:observer',{facing:'up',powered:'false'});
 world.set(8,0,0,'minecraft:redstone_lamp',{lit:'false'});
 world.finalize();
 const probes=[[0,2,0],[0,1,0],[0,0,0],[4,0,0]];
 const request={probes:[...probes,[8,2,0],[8,1,0],[8,0,0]],inventory_probes:probes,settle_ticks:1,samples:Array.from({length:81},(_,i)=>i),
  block_entities:[{at:[0,2,0],data_version:2975,snbt:'{Items:[{Slot:0b,id:"minecraft:iron_ingot",Count:4b}]}'},
   {at:[4,0,0],data_version:2975,snbt:'{Items:[{Slot:5b,id:"minecraft:stone",Count:7b,tag:{display:{Name:\'{"text":"Filter"}\'}}}]}'}],
  actions:[{tick:10,type:'use',at:[8,2,0]},{tick:40,type:'use',at:[8,2,0]}, {tick:0,type:'insert',at:[0,2,0],slot:1,item:'minecraft:gold_ingot',count:3},
   {tick:0,type:'insert',at:[4,0,0],slot:5,item:'minecraft:dirt',count:1},
   {tick:0,type:'extract',at:[4,0,0],slot:5,count:8},
   {tick:50,type:'extract',at:[4,0,0],slot:5,count:2}]};
 try {
  const reports=[];
  for(const [name,sprint,encoding] of [['sprint',true,'full'],['stock',false,'full'],['packed',true,'rle-v1']]){
   verifier.sprint=sprint;verifier.reportEncoding=encoding;
   const result=await verifier.run(world,request,{reportPath:path.join(root,name+'.json')});
   const report=JSON.parse(fs.readFileSync(result.reportFile)); reports.push(report);
   assert.equal(result.ok,true);assert.equal(report.inputs.filter(x=>!x.applied).length,2);
   assert.match(report.importedBlockEntities[1].snbt,/custom_name/);
   const last=report.observations.at(-1);
   assert.deepEqual(last.blocks[2].inventory.map(x=>[x.id,x.count]),[['minecraft:iron_ingot',4],['minecraft:gold_ingot',3]]);
   assert.deepEqual(last.blocks[0].inventory,[]);
   assert.equal(last.blocks[3].inventory[0].count,5);
   assert.match(last.blocks[4].block,/note=2/);
   assert.ok(report.observations.some(o=>/lit=true/.test(o.blocks[6].block)),'native note interaction must reach an observer and lamp');
   assert.match(last.blocks[3].inventory[0].snbt,/Filter/);
  }
  for(const key of ['initial','observations','inputs'])assert.deepEqual(reports[0][key],reports[1][key],key+' differs at stock versus sprint pacing');
  const expanded=reports[2].observations.flatMap(o=>Array.from({length:o.endTick-o.tick+1},(_,i)=>({tick:o.tick+i,blocks:o.blocks})));
  assert.deepEqual(expanded,reports[0].observations,'RLE must retain inventory-only changes');
  await assert.rejects(verifier.run(world,{...request,block_entities:[{at:[4,0,0],data_version:999999,snbt:'{}'}]}),/future/);
 }finally{await verifier.stop();}
});
