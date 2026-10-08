# SPDX-License-Identifier: Apache-2.0
"""Refusal checks prove Docker is not invoked, including a validly signed bad OCI."""
import pathlib,subprocess,json,uuid,os,tarfile,io,hashlib,shlex
# Operator supplies a previously recorded real command; paths/keys are never in Git.
import argparse
p=argparse.ArgumentParser();p.add_argument('--command',type=pathlib.Path,required=True);p.add_argument('--node',required=True);a=p.parse_args();args=json.loads(a.command.read_text());args[0]='python3'
parent=pathlib.Path(args[args.index('--output-parent')+1]);root=parent/('import-refusal-'+str(uuid.uuid4()));root.mkdir(mode=0o700)
marker=root/'docker-was-invoked';fake=root/'docker';fake.write_text('#!/bin/sh\ntouch '+shlex.quote(str(marker))+'\nexit 1\n');fake.chmod(0o700)
args[args.index('--docker')+1]=str(fake);checks=[]
def reject(label,command):
 r=subprocess.run(command,capture_output=True,text=True,timeout=90);assert r.returncode!=0 and not marker.exists(),label;checks.append(label)
bad=list(args);bad[bad.index('--source-commit')+1]='a'*40;reject('wrong source revision refuses before Docker',bad)
original=pathlib.Path(args[args.index('--archive')+1]);changed=root/'changed';changed.mkdir(mode=0o700);archive=changed/'runtime.tar'
with original.open('rb') as source,archive.open('xb') as target:
 while b:=source.read(65536):target.write(b)
with archive.open('r+b') as f:f.seek(4096);byte=f.read(1);f.seek(4096);f.write(bytes([byte[0]^1]))
archive.chmod(0o400);bad=list(args);bad[bad.index('--archive')+1]=str(archive);reject('same-size modified artifact refuses before Docker',bad)
# The compatibility manifest is outside OCI blobs; re-signing this malformed
# archive makes cryptographic verification pass but internal validation must fail.
foreign=root/'foreign-manifest';foreign.mkdir(mode=0o700);malformed=foreign/'runtime.tar'
with tarfile.open(original) as source,tarfile.open(malformed,'w') as target:
 for entry in source:
  if entry.name=='manifest.json':
   value=json.load(source.extractfile(entry));value[0]['Config']='blobs/sha256/'+'a'*64;b=json.dumps(value).encode();entry.size=len(b);target.addfile(entry,io.BytesIO(b))
  else:target.addfile(entry,source.extractfile(entry) if entry.isfile() else None)
malformed.chmod(0o400)
signed_root=root/'signed';signed_root.mkdir(mode=0o700)
node=r"""const fs=require('fs'),c=require('crypto');const [old,out,artifact]=process.argv.slice(1);const env=JSON.parse(fs.readFileSync(old));const r=JSON.parse(env.payload);const bytes=fs.readFileSync(artifact);r.artifact.bytes=bytes.length;r.artifact.sha256=c.createHash('sha256').update(bytes).digest('hex');const now=Math.floor(Date.now()/1000);r.issuedAt=now-5;r.expiresAt=now+3600;const k=c.generateKeyPairSync('ed25519'),raw=k.publicKey.export({type:'spki',format:'der'}).subarray(-32),hash=b=>c.createHash('sha256').update(b).digest('hex');const policy={format:1,channel:r.channel,target:r.target,protocolVersion:1,sourceSchemaSha256:r.sourceSchemas[0],minimumSequence:40,minimumIssuedAt:now-10,publicKeys:[raw.toString('hex')]};const payload=JSON.stringify(r),b=Buffer.from(payload),n=Buffer.alloc(8);n.writeBigUInt64BE(BigInt(b.length));const e={format:1,algorithm:'ed25519',keyId:hash(raw),payload,signature:c.sign(null,Buffer.concat([Buffer.from('ExhibitOS-runtime-release-v1\0'),n,b]),k.privateKey).toString('hex')};fs.writeFileSync(out+'/policy.json',JSON.stringify(policy),{mode:0o600,flag:'wx'});fs.writeFileSync(out+'/release.json',JSON.stringify(e),{mode:0o600,flag:'wx'});"""
subprocess.run([a.node,'-e',node,args[args.index('--release')+1],str(signed_root),str(malformed)],check=True)
cli=args[args.index('--cli')+1];policy=str(signed_root/'policy.json');release=str(signed_root/'release.json')
v=subprocess.run([cli,'verify','--policy',policy,'--release',release,'--artifact',str(malformed)],capture_output=True,text=True);assert v.returncode==0 and json.loads(v.stdout)['artifactVerified'],v.stdout
bad=list(args)
for flag,value in [('--policy',policy),('--release',release),('--archive',str(malformed))]:bad[bad.index(flag)+1]=value
reject('validly signed wrong Docker compatibility config refuses before Docker',bad)
assert hashlib.file_digest(original.open('rb'),'sha256').hexdigest()==json.loads(json.loads(pathlib.Path(args[args.index('--release')+1]).read_text())['payload'])['artifact']['sha256']
report={'format':1,'checks':checks,'dockerNeverInvoked':not marker.exists(),'malformedSignedArtifactVerifiedByRust':True,'limits':['fake Docker used for refusal checks, actual positive import tested separately','synthetic ephemeral process-only private key; malformed genuine copies retained privately']}
with (root/'report.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
(root/'report.json').chmod(0o600)
for check in checks:print('PASS '+check)
print('Report '+str(root/'report.json'))
