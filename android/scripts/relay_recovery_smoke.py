"""Verify actual Android sender recovery from an unavailable public relay."""
import base64,hashlib,json,os,subprocess,time,uuid
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];WORK=ROOT/'android/.tools/smoke';WORK.mkdir(exist_ok=True)
ADB='X:/asproj/platform-tools/adb.exe';CROC=str(ROOT/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
code=next('recovery-'+str(i) for i in range(1000) if hashlib.sha256(('recovery-'+str(i)).encode()).digest()[-1]%4==2)
cfg=dict(id='recovery-'+uuid.uuid4().hex[:8],action='send',mode='Croc',code=code,fixture=True)
args=[ADB,'shell','am','instrument','-w','-e','config',base64.b64encode(json.dumps(cfg).encode()).decode(),'com.vinaywinai.databeam.test/com.vinaywinai.databeam.SmokeInstrumentation']
phone=subprocess.Popen(args,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
lines=[]
try:
 replacement=None
 for line in phone.stdout:
  lines.append(line)
  print(line.rstrip(),flush=True)
  if 'replacement_code=' in line:replacement=line.split('replacement_code=',1)[1].strip();break
 if not replacement:raise RuntimeError('No replacement code generated')
 assert replacement!=code and hashlib.sha256(replacement.encode()).digest()[-1]%4!=2
 relay=str(hashlib.sha256(replacement.encode()).digest()[-1]%4+1)+'.getcroc.com:9009'
 dest=WORK/cfg['id'];dest.mkdir()
 result=subprocess.run([CROC,'--yes','--ignore-stdin','--relay',relay,'--out',str(dest)],env=dict(os.environ,CROC_SECRET=replacement),capture_output=True,text=True,timeout=75)
 (WORK/(cfg['id']+'-desktop.log')).write_text(result.stdout+result.stderr,encoding='utf-8')
 assert result.returncode==0,result.stdout+result.stderr
 target=next(dest.rglob('smoke-fixture.bin'))
 expected=hashlib.sha256(bytes(i%251 for i in range(32768))*32).hexdigest()
 assert hashlib.sha256(target.read_bytes()).hexdigest()==expected
 print('PASS actual sender relay recovery and file hash',flush=True)
finally:
 subprocess.run([ADB,'shell','am','force-stop','com.vinaywinai.databeam'],capture_output=True)
 phone.terminate()
 (WORK/(cfg['id']+'-android.log')).write_text(''.join(lines),encoding='utf-8')
