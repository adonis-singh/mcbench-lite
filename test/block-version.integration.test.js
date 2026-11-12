'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {World}=require('../server/world'),{MinecraftVerifier}=require('../server/minecraft-verify');
test('native block data migration preserves properties and matches current block behavior',{skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
 const root=path.resolve('data/verify-calibration/block-version'),v=new MinecraftVerifier({root,physicsGuard:false,placementMode:'strict',reportEncoding:'rle-v1'});
 const make=id=>{const w=new World();w.set(0,0,0,'minecraft:stone');w.set(0,1,0,id,{axis:'x',waterlogged:'false'});w.finalize();return w;};
 const q={probes:[[0,0,0],[0,1,0]],samples:Array.from({length:21},(_,i)=>i),settle_ticks:1};
 try{
  const run=async(w,extra,name)=>JSON.parse(fs.readFileSync((await v.run(w,{...q,...extra},{reportPath:path.join(root,name+'.json')})).reportFile));
  const a=await run(make('minecraft:chain'),{block_data_version:3218},'old');v.sprint=false;const b=await run(make('minecraft:iron_chain'),{},'current-stock');
  assert.equal(a.sourceBlockDataVersion,3218);assert.deepEqual(a.blockStateMigrations,[{before:'minecraft:chain[axis=x,waterlogged=false]',after:'minecraft:iron_chain[axis=x,waterlogged=false]'}]);assert.deepEqual(a.observations,b.observations);assert.equal(b.sourceBlockDataVersion,undefined);
  await assert.rejects(run(make('minecraft:chain'),{block_data_version:99999},'future'),/future block data version/);
 }finally{await v.stop();}
});
