'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {World}=require('../server/world'),{MinecraftVerifier}=require('../server/minecraft-verify');
test('saved block and fluid ticks retain native delays under stock and sprint',{skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
 const root=path.resolve('data/verify-calibration/scheduled-tick'),v=new MinecraftVerifier({root,physicsGuard:false,placementMode:'strict',reportEncoding:'rle-v1'}),w=new World();
 w.set(0,1,0,'minecraft:redstone_lamp',{lit:'true'});w.set(2,1,0,'minecraft:redstone_lamp',{lit:'true'});
 for(let x=4;x<8;x++)for(let z=0;z<3;z++)w.set(x,0,z,'minecraft:stone');
 for(let x=4;x<8;x++){w.set(x,1,0,'minecraft:stone');w.set(x,1,2,'minecraft:stone');}w.set(4,1,1,'minecraft:stone');
 w.set(5,1,1,'minecraft:water',{level:'0'});w.finalize();
 const saved={bounds:[[0,0,0],[7,2,2]],data_version:4189,entries:[{kind:'block',id:'minecraft:redstone_lamp',at:[0,1,0],delay:10,priority:-1,sub_tick:'9007199254740993'},{kind:'block',id:'minecraft:redstone_lamp',at:[2,1,0],delay:15,priority:2,sub_tick:'9007199254740994'},{kind:'fluid',id:'minecraft:water',at:[5,1,1],delay:12,priority:0,sub_tick:'-2'}]};
 const req={probes:[[0,1,0],[2,1,0],[6,1,1]],samples:Array.from({length:22},(_,i)=>i),settle_ticks:1,scheduled_ticks:saved};
 const expand=r=>r.observations.flatMap(o=>Array.from({length:(o.endTick??o.tick)-o.tick+1},(_,i)=>({tick:o.tick+i,blocks:o.blocks})));
 try{
  const run=async(name,q=req)=>JSON.parse(fs.readFileSync((await v.run(w,q,{reportPath:path.join(root,name+'.json')})).reportFile));
  const fast=await run('sprint');v.sprint=false;const stock=await run('stock');assert.deepEqual(expand(fast),expand(stock));assert.deepEqual(fast.importedScheduledTicks,saved);
  const o=expand(fast);assert.equal(o.find(x=>x.blocks[0].block.includes('lit=false')).tick,9);assert.equal(o.find(x=>x.blocks[1].block.includes('lit=false')).tick,14);
  assert.equal(o.find(x=>x.blocks[2].block.startsWith('minecraft:water')).tick,11);
  const empty=await run('empty',{...req,scheduled_ticks:{...saved,entries:[]}});assert.ok(expand(empty).every(x=>x.blocks[0].block.includes('lit=true')&&x.blocks[1].block.includes('lit=true')&&x.blocks[2].block==='minecraft:air'));
  await assert.rejects(run('unknown',{...req,scheduled_ticks:{...saved,entries:[{...saved.entries[0],id:'minecraft:not_a_block'}]}}),/Unknown saved block tick/);
 }finally{await v.stop();}
});
