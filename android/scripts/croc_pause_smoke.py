import sys
sys.stdout.reconfigure(encoding="utf-8")
from pathlib import Path
import subprocess,os,base64,json,uuid,hashlib,time
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';A='X:/asproj/platform-tools/adb.exe';C=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
f=W/'pause-256MiB.bin';assert f.exists();token=uuid.uuid4().hex[:8];code='pause-croc-'+token;procs=[]
log=(W/(token+'-croc-pause.log')).open('w')
try:
 relay=subprocess.Popen([C,'relay','--host','127.0.0.1','--ports','39009,39010,39011,39012,39013'],stdout=log,stderr=log);procs.append(relay)
 for p in range(39009,39014):subprocess.run([A,'reverse',f'tcp:{p}',f'tcp:{p}'],check=True,capture_output=True)
 time.sleep(.5)
 sender=subprocess.Popen([C,'--yes','--ignore-stdin','--relay','127.0.0.1:39009','--throttleUpload','20m','send','--',str(f)],env=dict(os.environ,CROC_SECRET=code),stdout=log,stderr=log);procs.append(sender)
 cfg=dict(id='croc-pause-'+token,mode='Croc',action='receive',code=code,relay='127.0.0.1:39009',pauseTest=True)
 result=subprocess.run([A,'shell','am','instrument','-w','-e','config',base64.b64encode(json.dumps(cfg).encode()).decode(),'com.vinaywinai.databeam.test/com.vinaywinai.databeam.SmokeInstrumentation'],capture_output=True,text=True,encoding='utf-8',timeout=145)
 (W/(token+'-croc-android.log')).write_text(result.stdout+result.stderr,encoding='utf-8')
 print(result.stdout[-3500:].encode("ascii","backslashreplace").decode());expected='0c5ebcf5cb0e4611da9ecf2adf97f248001e00ed6c0bd75a3a44a33ff26d3612'
 assert 'INSTRUMENTATION_CODE: 0' in result.stdout
 assert 'download_hash_pause-256MiB.bin='+expected in result.stdout
 print('PASS Croc retained-connection pause/resume and automatic Downloads hash')
finally:
 subprocess.run([A,'shell','am','force-stop','com.vinaywinai.databeam'],capture_output=True)
 for p in procs:
  if p.poll() is None:p.terminate()
 for p in range(39009,39014):subprocess.run([A,'reverse','--remove',f'tcp:{p}'],capture_output=True)
 log.close()
