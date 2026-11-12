'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {execFileSync}=require('node:child_process');
test('same conversation ID in recovered roots keeps Bash and build files aligned, and stale mounts are replaced',{skip:process.env.MCBENCH_SANDBOX_INTEGRATION!=='1',timeout:120000},async()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'mcb-root-recovery-')),previous=process.env.MCBENCH_DATA_DIR;
 // Each load stands for a process of its own, data folder and storage included.
 const load=directory=>{process.env.MCBENCH_DATA_DIR=directory;for(const name of ['sandbox','storage'])delete require.cache[require.resolve('../server/'+name)];return require('../server/sandbox');};
 const a=load(path.join(root,'original')),b=load(path.join(root,'resumed')),id='recovery-'+process.pid;
 try{
  a.writeWorkFile(id,'identity.txt','original');b.writeWorkFile(id,'identity.txt','resumed');
  assert.notEqual(a._internal.containerName(id),b._internal.containerName(id));
  const first=await a.exec(id,'cat identity.txt',5000);assert.match(first.formatted,/original/);
  const second=await b.exec(id,'cat identity.txt; printf model-generated > generated.txt',5000);assert.match(second.formatted,/resumed/);
  assert.equal(fs.readFileSync(b.resolveWorkPath(id,'generated.txt'),'utf8'),'model-generated');assert.equal(fs.existsSync(a.resolveWorkPath(id,'generated.txt')),false);
  await b.reset(id);
  const name=b._internal.containerName(id),image=execFileSync('docker',['inspect','-f','{{.Config.Image}}',a._internal.containerName(id)],{encoding:'utf8'}).trim();
  execFileSync('docker',['run','-d','--name',name,'--network','none','--stop-timeout','0','-v',a.workDir(id)+':/work',image]);
  const repaired=await b.exec(id,'cat identity.txt; cat generated.txt',5000);assert.equal(repaired.exitCode,0);assert.match(repaired.formatted,/resumed/);assert.match(repaired.formatted,/model-generated/);
  const old=await a.exec(id,'cat identity.txt',5000);assert.match(old.formatted,/original/);
 }finally{
  await a.destroy(id);await b.destroy(id);fs.rmSync(root,{recursive:true,force:true});
  if(previous===undefined)delete process.env.MCBENCH_DATA_DIR;else process.env.MCBENCH_DATA_DIR=previous;
  delete require.cache[require.resolve('../server/sandbox')];
 }
});
