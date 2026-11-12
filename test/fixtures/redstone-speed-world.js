'use strict';
const { World } = require('../../server/world');
function fixture() {
  const w = new World();
  w.fill([-2,0,-2],[34,0,100],'minecraft:stone');
  const probes=[], actions=[];
  for(let z=0;z<96;z+=3) {
    w.set(0,1,z,'minecraft:lever',{face:'floor',facing:'east',powered:'false'});
    for(let x=1;x<30;x++) w.set(x,1,z,x===14?'minecraft:repeater':'minecraft:redstone_wire',x===14?{facing:'east',delay:'1',powered:'false'}:{});
    w.set(30,1,z,'minecraft:redstone_lamp',{lit:'false'});
    probes.push([13,1,z],[30,1,z]);
    for(let tick=0;tick<2000;tick+=20) actions.push({tick,type:'toggle',at:[0,1,z]});
  }
  // Native piston motion, observer pulses, and powered rails beside the wire fixture.
  w.set(0,1,98,'minecraft:lever',{face:'floor',facing:'east',powered:'false'});
  w.set(1,1,98,'minecraft:sticky_piston',{facing:'east',extended:'false'});
  w.set(2,1,98,'minecraft:stone');
  w.set(3,1,99,'minecraft:observer',{facing:'north',powered:'false'});
  for(let x=7;x<16;x++)w.set(x,1,98,'minecraft:powered_rail',{shape:'east_west',powered:'false'});
  w.set(6,1,98,'minecraft:lever',{face:'floor',facing:'east',powered:'false'});
  for(let x=0;x<17;x++)probes.push([x,1,98]);
  probes.push([3,1,99]);
  for(let tick=0;tick<2000;tick+=20)for(const x of [0,6])actions.push({tick,type:'toggle',at:[x,1,98]});
  w.finalize(); return {w,request:{probes,actions,samples:Array.from({length:2001},(_,i)=>i)}};
}

module.exports = { fixture };
