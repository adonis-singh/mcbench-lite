'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const {execFileSync}=require('node:child_process');
const {compilePlugin}=require('../server/minecraft-verify');
const {MC_VERSION}=require('../server/assets');
test('compiled verifier reuse crosses runtime roots and invalidates on source or dependency changes',async()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'verify-plugin-cache-')),previous=process.env.MCBENCH_JAVAC;
 try{
  const compiler=execFileSync('which',['javac'],{encoding:'utf8'}).trim(),wrapper=path.join(root,'javac'),log=path.join(root,'compilations');
  fs.writeFileSync(wrapper,`#!/bin/sh\nif [ "$1" != "-version" ]; then echo build >> '${log}'; fi\nexec '${compiler}' "$@"\n`,{mode:0o755});process.env.MCBENCH_JAVAC=wrapper;
  const cacheDir=path.join(root,'cache');const empty=Buffer.alloc(22);empty.writeUInt32LE(0x06054b50);
  function runtime(name,extra=''){
   const dir=path.join(root,name),server=path.join(dir,'versions',MC_VERSION);fs.mkdirSync(server,{recursive:true});fs.writeFileSync(path.join(server,`paper-${MC_VERSION}.jar`),empty);
   const libs=path.join(dir,'libraries');fs.mkdirSync(libs);const jar=Buffer.concat([empty,Buffer.from(extra)]);jar.writeUInt16LE(Buffer.byteLength(extra),20);fs.writeFileSync(path.join(libs,'fixture.jar'),jar);return dir;
  }
  const sources={'McbenchVerify.java':'public class McbenchVerify { public int value() { return 1; } }','McbenchExperiment.java':'public class McbenchExperiment {}'};
  const a=runtime('a');await compilePlugin(a,{cacheDir,sources});
  const b=runtime('b');await compilePlugin(b,{cacheDir,sources});
  const count=()=>fs.readFileSync(log,'utf8').trim().split('\n').length;
  assert.equal(count(),1);assert.deepEqual(fs.readFileSync(path.join(a,'plugins/mcbench-verify.jar')),fs.readFileSync(path.join(b,'plugins/mcbench-verify.jar')));
  await compilePlugin(runtime('c'),{cacheDir,sources:{...sources,'McbenchVerify.java':sources['McbenchVerify.java'].replace('return 1','return 2')}});assert.equal(count(),2);
  await compilePlugin(runtime('d','changed dependency'),{cacheDir,sources});assert.equal(count(),3);
 }finally{if(previous===undefined)delete process.env.MCBENCH_JAVAC;else process.env.MCBENCH_JAVAC=previous;fs.rmSync(root,{recursive:true,force:true});}
});
