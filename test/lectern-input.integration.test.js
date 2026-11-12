'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path');
const {World}=require('../server/world'),{MinecraftVerifier}=require('../server/minecraft-verify');
test('native lectern pages preserve book, signal levels and pulse timing under stock and sprint',{skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
 const root=path.resolve('data/verify-calibration/lectern-input'),v=new MinecraftVerifier({root,physicsGuard:false,reportEncoding:'rle-v1'}),w=new World();
 for(let x=0;x<3;x++)w.set(x,0,0,'minecraft:stone');w.set(0,1,0,'minecraft:lectern',{facing:'north',has_book:'true',powered:'false'});w.set(1,1,0,'minecraft:comparator',{facing:'west',mode:'compare',powered:'false'});w.set(2,1,0,'minecraft:redstone_wire');w.finalize();
 const pages=Array.from({length:15},(_,i)=>JSON.stringify(JSON.stringify(String(i)))).join(',');
 const request={probes:[[0,1,0],[1,1,0],[2,1,0]],samples:Array.from({length:96},(_,i)=>i),settle_ticks:20,block_entities:[{at:[0,1,0],data_version:4189,snbt:`{id:"minecraft:lectern",Page:0,Book:{id:"minecraft:written_book",count:1,components:{"minecraft:written_book_content":{title:"levels",author:"Verifier",pages:[${pages}],resolved:true}}}}`}],actions:[{tick:5,type:'page',at:[0,1,0],page:14},{tick:15,type:'page',at:[0,1,0],page:7},{tick:25,type:'page',at:[0,1,0],page:0},{tick:35,type:'page',at:[0,1,0],page:15},{tick:45,type:'page',at:[0,1,0],page:0},{tick:55,type:'book',at:[0,1,0],present:false},{tick:65,type:'page',at:[0,1,0],page:7},{tick:75,type:'book',at:[0,1,0],present:true},{tick:75,type:'page',at:[0,1,0],page:7}]};
 const expand=r=>r.observations.flatMap(o=>Array.from({length:(o.endTick??o.tick)-o.tick+1},(_,i)=>({...o,tick:o.tick+i})));
 try{
  const run=async(name)=>{const r=await v.run(w,request,{reportPath:path.join(root,name+'.json')});return JSON.parse(fs.readFileSync(r.reportFile));};
  const fast=await run('sprint');v.sprint=false;const stock=await run('stock');const a=expand(fast),b=expand(stock);
  assert.deepEqual(a.map(o=>o.blocks),b.map(o=>o.blocks));assert.deepEqual(fast.inputs.map(i=>i.applied),[true,true,true,false,true,true,false,true,true]);assert.match(fast.inputs[3].error,/outside this book/);
  for(const [tick,power]of [[10,15],[20,8],[30,1],[40,1],[50,1],[60,0],[70,0],[80,8],[90,8]])assert.match(a.find(o=>o.tick===tick).blocks[2].block,new RegExp(`power=${power}(,|\\])`));
  assert.ok(a.some(o=>/powered=true/.test(o.blocks[0].block)),'native page pulse must occur');
  assert.ok(a.filter(o=>o.tick>=35&&o.tick<55).every(o=>/powered=false/.test(o.blocks[0].block)),'invalid/unchanged page must not create a pulse');
  const page=await v.runtime.command('execute in minecraft:mcbench_test run data get block 0 65 0 Page',false);assert.match(page,/7/);
  const book=await v.runtime.command('execute in minecraft:mcbench_test run data get block 0 65 0 Book.id',false);assert.match(book,/minecraft:written_book/);
 }finally{await v.stop();}
});
