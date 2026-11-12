'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { promisify } = require('node:util');
const exec = promisify(require('node:child_process').execFile);
const { test } = require('node:test');
const { MinecraftVerifier } = require('../server/minecraft-verify');

function jars(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap(e => e.isDirectory() ? jars(path.join(dir, e.name)) : e.name.endsWith('.jar') ? [path.join(dir, e.name)] : []);
}
const { fixture } = require('./fixtures/redstone-speed-world');

test('physics guard preserves stock observations and dynamic listener cancellation', {skip:process.env.MCBENCH_VERIFY_INTEGRATION!=='1',timeout:600000},async()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'mcbench-physics-parity-'));
  const {w,request}=fixture(); const measurements=[]; const traces=[]; const counts=[];
  try {
    for(const physicsGuard of [false,true]) {
      const directory=path.join(root,physicsGuard?'guard':'stock');
      let v=new MinecraftVerifier({root:directory,physicsGuard,reportEncoding:'rle-v1'});
      try {
        // Prepare dependencies, then add the integration-only listener plugin.
        await v.run(w,{probes:[[0,1,0]],samples:[0]}); await v.stop();
        const paper=path.join(directory,'paper'), build=path.join(directory,'listener-build');fs.mkdirSync(build);
        await exec('javac',['--release','21','-proc:none','-cp',jars(path.join(paper,'libraries')).join(path.delimiter),'-d',build,path.join(__dirname,'fixtures/PhysicsListenerProbe.java')]);
        fs.writeFileSync(path.join(build,'plugin.yml'),"name: PhysicsListenerProbe\nversion: '1'\nmain: PhysicsListenerProbe\napi-version: '1.21'\ncommands:\n  physicsprobe:\n    description: Integration-only listener control\n");
        await exec('jar',['cf',path.join(paper,'plugins/physics-listener-probe.jar'),'-C',build,'.']);
        if (physicsGuard) {
          const unknown=path.join(directory,'unknown');
          const classDir=path.join(unknown,'net/minecraft/world/level/redstone');fs.mkdirSync(classDir,{recursive:true});
          fs.writeFileSync(path.join(classDir,'NeighborUpdater.class'),'unapproved class bytes');
          const fakeJar=path.join(directory,'unknown.jar');
          await exec('jar',['cf',fakeJar,'-C',unknown,'.']);
          const supported=await require('../server/native-speed').physicsGuardBuilder()(path.join(directory,'unsupported-runtime'),fakeJar,jars(path.join(paper,'libraries')));
          assert.equal(supported,false,'unknown server bytecode must fall back without a patch');
        }
        v=new MinecraftVerifier({root:directory,physicsGuard,reportEncoding:'rle-v1'});
        await v.run(w,{probes:[[0,1,0]],samples:[0]});
        const modes=['off','count','cancel','off'];const modeTraces=[],modeCounts=[];
        for(const mode of modes) {
          await v.runtime.command(`physicsprobe ${mode==='count'?'on':mode}`);
          const r=await v.run(w,request);assert.equal(r.physicsGuard,physicsGuard?'listener-guard-v1':'stock');
          const report=JSON.parse(fs.readFileSync(r.reportFile));
          modeTraces.push({initial:report.initial,observations:report.observations,inputs:report.inputs,preview:report.preview});
          modeCounts.push(Number((await v.runtime.command('physicsprobe count')).match(/PHYSICS_COUNT (\d+)/)[1]));
          measurements.push({physicsGuard,mode,simulationMs:r.timings.simulationMs,wallMs:r.wallMs,probeReadMs:r.metrics.probeReadMs});
        }
        traces.push(modeTraces);counts.push(modeCounts);
      } finally {await v.stop();}
    }
    assert.deepEqual(traces[1],traces[0],'every sampled state, signal, input and preview must equal stock');
    assert.deepEqual(counts[1],counts[0],'listeners must receive exactly the original number of events');
    assert.ok(counts[0][1]>0 && counts[0][2]>0);assert.equal(counts[0][0],0);assert.equal(counts[0][3],0);
    assert.notDeepEqual(traces[0][1].observations,traces[0][2].observations,'cancelling listeners must actually affect native updates');
    assert.deepEqual(traces[0][0],traces[0][3],'removing listeners restores normal native results');
    fs.writeFileSync(path.join(os.tmpdir(),'mcbench-physics-guard-results.json'),JSON.stringify({measurements,counts,parity:true},null,2));
    console.log(JSON.stringify(measurements));
  } finally {fs.rmSync(root,{recursive:true,force:true});}
});
