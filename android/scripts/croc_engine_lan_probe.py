"""Diagnostic Croc engine transfer under the app UID, without the test Activity lifecycle."""
from pathlib import Path
import subprocess,hashlib,os,re,uuid,sys,shlex
sys.stdout.reconfigure(encoding='utf-8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';A='X:/asproj/platform-tools/adb.exe';P='com.vinaywinai.databeam'
C=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
package=subprocess.check_output([A,'shell','dumpsys','package',P],text=True)
library=re.search(r'legacyNativeLibraryDir=(\S+)',package).group(1)+'/arm64/libcroc.so'
token=uuid.uuid4().hex[:8];code='engine-lan-'+token
while hashlib.sha256(code.encode()).digest()[-1]%4!=0:code+='x'
destination=W/('engine-lan-'+token);destination.mkdir()
desktop_log=(W/('engine-lan-'+token+'-desktop.log')).open('w',encoding='utf-8')
phone_log=(W/('engine-lan-'+token+'-phone.log')).open('w',encoding='utf-8')
desktop=subprocess.Popen([C,'--debug','--yes','--ignore-stdin','--relay','1.getcroc.com:9009','--out',str(destination)],env=dict(os.environ,CROC_SECRET=code),stdout=desktop_log,stderr=desktop_log)
command='export CROC_SECRET='+shlex.quote(code)+' HOME=/data/user/0/'+P+'/files TMPDIR=/data/user/0/'+P+'/cache; exec '+shlex.join([library,'--debug','--yes','--ignore-stdin','--relay','1.getcroc.com:9009','send','--','/data/user/0/'+P+'/files/smoke-fixture.bin'])
phone=subprocess.Popen([A,'shell',shlex.join(['run-as',P,'/system/bin/sh','-c',command])],stdout=phone_log,stderr=phone_log)
try:
 desktop.wait(timeout=75);phone.wait(timeout=15)
 assert desktop.returncode==0 and phone.returncode==0,'Inspect engine-lan logs'
 payload=next(destination.rglob('smoke-fixture.bin'))
 with payload.open('rb') as file:actual=hashlib.file_digest(file,'sha256').hexdigest()
 assert actual=='474a99dd2796b7fd2547f6ca3aef4335bc7d6f5a21c12ebd4b76c8818a2e9919'
 print('PASS standalone app-UID Croc 512 MiB LAN payload, SHA256',actual,flush=True)
finally:
 for process in (desktop,phone):
  if process.poll() is None:process.terminate()
 desktop_log.close();phone_log.close()
