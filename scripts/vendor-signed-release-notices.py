#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Exact Cargo.lock closure licenses; checks registry archive checksums, no source vendoring."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tomllib
import urllib.request
import argparse
p=argparse.ArgumentParser();p.add_argument('--cargo-home',required=True);a=p.parse_args()
r=Path(__file__).resolve().parent.parent
lock=tomllib.loads((r/'Cargo.lock').read_text())['package']
by={(p['name'],p['version']):p for p in lock};names={}
for k in by:names.setdefault(k[0],[]).append(k)
def lookup(s):
 x=s.split();k=(x[0],x[1]) if len(x)>1 else names[x[0]][0]
 if len(x)==1:assert len(names[x[0]])==1,'ambiguous lock dependency'
 return k
pending=[('ed25519-dalek','2.2.0')];closure=set()
while pending:
 k=pending.pop()
 if k in closure:continue
 closure.add(k);pending += [lookup(s) for s in by[k].get('dependencies',[])]
dest=r/'licenses/signed-release';dest.mkdir(parents=True,exist_ok=True)
cache=Path(a.cargo_home)/'registry/cache';inventory=[]
for k in sorted(closure):
 package=by[k];name,version=k;checksum=package['checksum'];filename=f'{name}-{version}.crate'
 hits=list(cache.glob('*/'+filename))
 if hits:data=hits[0].read_bytes()
 else:
  with urllib.request.urlopen(f'https://static.crates.io/crates/{name}/{filename}',timeout=60) as response:data=response.read(8*1024*1024+1)
 assert len(data)<=8*1024*1024 and hashlib.sha256(data).hexdigest()==checksum,'archive checksum/limit mismatch'
 prefix=f'{name}-{version}/';files=[]
 with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as archive:
  source=tomllib.loads(archive.extractfile(prefix+'Cargo.toml').read().decode())['package']
  for member in archive.getmembers():
   relative=member.name.removeprefix(prefix)
   if '/' in relative or not member.isfile() or not relative.upper().startswith(('LICENSE','COPYING','NOTICE')):continue
   assert member.size<=1024*1024
   body=archive.extractfile(member).read();target=f'{name}-{version}-{relative}';(dest/target).write_bytes(body)
   files.append({'name':target,'sha256':hashlib.sha256(body).hexdigest()})
 assert files,'missing upstream license originals'
 inventory.append({'name':name,'version':version,'license':source.get('license'),'checksum':checksum,'files':files,'source':f'https://crates.io/crates/{name}/{version}'})
(dest/'inventory.json').write_text(json.dumps(inventory,indent=2)+'\n')
print('PASS exact upstream licenses for',len(inventory),'locked dependency-closure crates;',sum(len(e['files']) for e in inventory),'original files')
