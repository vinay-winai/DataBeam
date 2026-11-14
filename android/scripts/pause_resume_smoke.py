import sys
sys.stdout.reconfigure(encoding="utf-8")
"""Serial real-device large-file pause/resume and automatic Downloads verification."""
from pathlib import Path
import subprocess,os,hashlib,json,base64,uuid,time,sys
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';W.mkdir(exist_ok=True)
ADB='X:/asproj/platform-tools/adb.exe';PEER=str(R/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe');CROC=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
f=W/'pause-256MiB.bin';chunk=bytes(i%251 for i in range(1024*1024));h=hashlib.sha256()
with f.open('wb') as out:
 for i in range(256):out.write(chunk);h.update(chunk)
expected=h.hexdigest()
for mode in sys.argv[1:] or ['Sendme','EasySendMe']:
 token=uuid.uuid4().hex[:8];procs=[];files=[];ports=[]
 try:
  log=(W/(token+'-peer.log')).open('w');files.append(log)
  peer=subprocess.Popen([PEER,'send',str(f),str(W/(token+'-cache'))],stdout=subprocess.PIPE,stderr=log,text=True);procs.append(peer)
  ticket=peer.stdout.readline().strip().removeprefix('TICKET=');assert ticket.startswith('blob'),ticket
  config=dict(id='pause-'+token,mode=mode,action='receive',pauseTest=True,ticket=ticket)
  if mode=='EasySendMe':
   config.pop('ticket');config.update(code='pause-test-'+token,relay='127.0.0.1:39009')
   log=(W/(token+'-croc.log')).open('w');files.append(log)
   relay=subprocess.Popen([CROC,'relay','--host','127.0.0.1','--ports','39009,39010,39011,39012,39013'],stdout=log,stderr=log);procs.append(relay)
   for port in range(39009,39014):subprocess.run([ADB,'reverse',f'tcp:{port}',f'tcp:{port}'],check=True,capture_output=True);ports.append(port)
   time.sleep(.5)
   handoff=subprocess.Popen([CROC,'--yes','--ignore-stdin','--relay',config['relay'],'send','--text',ticket],env=dict(os.environ,CROC_SECRET=config['code']),stdout=log,stderr=log);procs.append(handoff)
  encoded=base64.b64encode(json.dumps(config).encode()).decode()
  result=subprocess.run([ADB,'shell','am','instrument','-w','-e','config',encoded,'com.vinaywinai.databeam.test/com.vinaywinai.databeam.SmokeInstrumentation'],capture_output=True,text=True,encoding='utf-8',timeout=150)
  (W/(token+'-pause-resume.log')).write_text(result.stdout+result.stderr,encoding='utf-8')
  print(result.stdout[-3500:],flush=True)
  assert 'INSTRUMENTATION_CODE: 0' in result.stdout,result.stdout[-2500:]
  assert 'download_hash_pause-256MiB.bin='+expected in result.stdout,'Automatically saved file mismatch'
  assert 'hash_pause-256MiB.bin='+expected in result.stdout,'Internal file mismatch'
  print('PASS',mode,'real pause, nonzero resume and automatic Downloads SHA256',expected,flush=True)
 finally:
  subprocess.run([ADB,'shell','am','force-stop','com.vinaywinai.databeam'],capture_output=True)
  for p in reversed(procs):
   if p.poll() is None:p.terminate()
  for port in ports:subprocess.run([ADB,'reverse','--remove',f'tcp:{port}'],capture_output=True)
  for file in files:file.close()
