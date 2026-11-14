from pathlib import Path
import subprocess,json,base64,uuid,sys
sys.stdout.reconfigure(encoding='utf-8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';A='X:/asproj/platform-tools/adb.exe';P=str(R/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe');file=W/'pause-256MiB.bin';token='changed-'+uuid.uuid4().hex[:8];procs=[]
log=(W/(token+'-peer.log')).open('w')
def sender(cache):
 p=subprocess.Popen([P,'send',str(file),str(W/cache)],stdout=subprocess.PIPE,stderr=log,text=True);procs.append(p)
 t=p.stdout.readline().strip().removeprefix('TICKET=');assert t.startswith('blob');return p,t
try:
 first,ticket=sender(token+'-first')
 cfg=dict(id=token,mode='Sendme',action='receive',pauseTest=True,replaceTicket=True,ticket=ticket)
 cmd=[A,'shell','am','instrument','-w','-e','config',base64.b64encode(json.dumps(cfg).encode()).decode(),'com.vinaywinai.databeam.test/com.vinaywinai.databeam.SmokeInstrumentation']
 phone=subprocess.Popen(cmd,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,encoding='utf-8');procs.append(phone);lines=[]
 for line in phone.stdout:
  lines.append(line)
  if 'paused_cache_bytes=' in line:
   first.terminate();first.wait(timeout=10)
   second,new=sender(token+'-second');assert new!=ticket
   subprocess.run([A,'shell','run-as','com.vinaywinai.databeam','tee',f'files/replacement-{token}.txt'],input=new+'\n',text=True,encoding='utf-8',check=True,capture_output=True)
 text=''.join(lines);(W/(token+'-android.log')).write_text(text,encoding='utf-8')
 assert 'INSTRUMENTATION_CODE: 0' in text,text[-3000:]
 assert 'download_hash_pause-256MiB.bin=0c5ebcf5cb0e4611da9ecf2adf97f248001e00ed6c0bd75a3a44a33ff26d3612' in text
 print('PASS sender restart with changed ticket, partial cache reused, automatic Downloads hash')
finally:
 subprocess.run([A,'shell','am','force-stop','com.vinaywinai.databeam'],capture_output=True)
 for p in procs:
  if p.poll() is None:p.terminate()
 log.close()
