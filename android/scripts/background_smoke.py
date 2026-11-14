"""Serial screen-off/background checks using generated fixtures and real transfer engines."""
from pathlib import Path
import base64,hashlib,json,os,subprocess,sys,time,uuid,xml.etree.ElementTree as ET
sys.stdout.reconfigure(encoding='utf-8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke'
A='X:/asproj/platform-tools/adb.exe';C=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
P=str(R/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe')
RUNNER='com.vinaywinai.databeam.test/com.vinaywinai.databeam.SmokeInstrumentation'
MIB=512
block=bytes(i%251 for i in range(32768))*32
fixture=W/'background-512MiB.bin';digest=hashlib.sha256()
with fixture.open('wb') as f:
 for _ in range(MIB):f.write(block);digest.update(block)
expected=digest.hexdigest()
def adb(*args):return subprocess.run([A,*args],capture_output=True,text=True,encoding='utf-8',errors='replace',check=True).stdout
def state():
 xml=adb('shell','run-as','com.vinaywinai.databeam','cat','shared_prefs/transfer.xml')
 return json.loads(next((n.text for n in ET.fromstring(xml) if n.attrib.get('name')=='state'),'{}'))
checks=[tuple(arg.split(':')) for arg in sys.argv[1:]] or [(m,d) for m in ['Sendme','EasySendMe','Croc'] for d in ['receive','send']]
for mode,direction in checks:
 token=uuid.uuid4().hex[:8];procs=[];files=[];ports=[];phone=None
 test_mib=1 if direction=='send' and mode!='Croc' else MIB
 test_hash=hashlib.sha256(block).hexdigest() if test_mib==1 else expected
 def spawn(args,name,pipe=False,env=None):
  log=(W/(token+'-'+name+'.log')).open('w',encoding='utf-8');files.append(log)
  p=subprocess.Popen(args,stdout=subprocess.PIPE if pipe else log,stderr=log,text=True,env=env);procs.append(p);return p
 try:
  print('START',mode,direction,flush=True)
  adb('shell','input','keyevent','224');adb('shell','wm','dismiss-keyguard')
  cfg=dict(id='background-'+token,mode=mode,action=direction,code='background-'+token,relay='127.0.0.1:39009',backgroundTest=True)
  if mode!='Sendme':
   spawn([C,'relay','--host','127.0.0.1','--ports','39009,39010,39011,39012,39013'],'relay')
   for port in range(39009,39014):adb('reverse',f'tcp:{port}',f'tcp:{port}');ports.append(port)
   time.sleep(.5)
  if direction=='receive':
   if mode=='Croc':spawn([C,'--yes','--ignore-stdin','--relay',cfg['relay'],'--throttleUpload','20m','send','--',str(fixture)],'sender',env=dict(os.environ,CROC_SECRET=cfg['code']))
   else:
    peer=spawn([P,'send',str(fixture),str(W/(token+'-cache'))],'sender',True)
    ticket=peer.stdout.readline().strip().removeprefix('TICKET=');assert ticket.startswith('blob'),ticket
    if mode=='Sendme':cfg['ticket']=ticket
    else:spawn([C,'--yes','--ignore-stdin','--relay',cfg['relay'],'send','--text',ticket],'handoff',env=dict(os.environ,CROC_SECRET=cfg['code']))
  else:cfg.update(fixture=True,fixtureMiB=test_mib,backgroundThreshold=65536 if test_mib==1 else 8*1024*1024)
  encoded=base64.b64encode(json.dumps(cfg).encode()).decode()
  phone_log=W/(token+'-phone.log');handle=phone_log.open('w',encoding='utf-8');files.append(handle)
  phone=subprocess.Popen([A,'shell','am','instrument','-w','-e','config',encoded,RUNNER],stdout=handle,stderr=handle);procs.append(phone)
  if direction=='receive':
   phone.wait(timeout=145);result=phone_log.read_text(encoding='utf-8')
   assert 'INSTRUMENTATION_CODE: 0' in result,result[-2500:]
   assert 'screen_off=true' in result,result[-2500:]
   assert 'download_hash_background-512MiB.bin='+expected in result,'Public saved-file hash mismatch'
   final=state();assert final['status']=='complete'
  else:
   end=time.monotonic()+65
   while time.monotonic()<end:
    current=state()
    if current.get('id')==cfg['id'] and (mode=='Croc' or current.get('ticket')):break
    time.sleep(.25)
   if mode=='EasySendMe':
    handoff=subprocess.run([C,'--yes','--ignore-stdin','--relay',cfg['relay']],env=dict(os.environ,CROC_SECRET=cfg['code']),capture_output=True,text=True,timeout=50)
    assert current['ticket'] in handoff.stdout,'Short-code handoff failed'
   dest=W/(token+'-received');dest.mkdir()
   if mode=='Croc':cmd=[C,'--yes','--ignore-stdin','--relay',cfg['relay'],'--out',str(dest)]
   else:cmd=[P,'receive',current['ticket'],str(dest),str(W/(token+'-receiver-cache'))]
   receiver=spawn(cmd,'receiver',env=dict(os.environ,CROC_SECRET=cfg['code']))
   receiver.wait(timeout=110);assert receiver.returncode==0,'Desktop receiver failed'
   received=next(dest.rglob('smoke-fixture.bin'))
   with received.open('rb') as f:actual=hashlib.file_digest(f,'sha256').hexdigest()
   assert actual==test_hash,'Desktop saved-file hash mismatch'
   result=phone_log.read_text(encoding='utf-8');assert 'background_started_' in result,'Did not enter background before completion'
   power=adb('shell','dumpsys','power');assert any('mWakefulness='+value in power for value in ['Asleep','Dozing']),'Screen not off'
   final=state()
  path=final.get('route',final.get('senderRoutes',{}))
  if not path:
   path=[json.loads(line.partition('send-route ')[2])['route'] for line in final.get('log','').splitlines() if line.startswith('send-route ')]
  values=list(path.values()) if isinstance(path,dict) else path if isinstance(path,list) else [path]
  assert any(value in ['direct','relay'] for value in values),'No actual payload-route evidence was reported'
  (W/(token+'-verified.json')).write_text(json.dumps(dict(mode=mode,direction=direction,mib=test_mib,sha256=test_hash,route=path,status=final['status'],screen_off=True),indent=2))
  print('PASS',mode,direction,f'{test_mib} MiB; Home + screen locked; SHA256',test_hash,'route',path,flush=True)
 finally:
  adb('shell','am','force-stop','com.vinaywinai.databeam')
  for p in reversed(procs):
   if p.poll() is None:p.terminate()
  for p in procs:
   try:p.wait(timeout=10)
   except subprocess.TimeoutExpired:p.kill()
  for file in files:file.close()
  for port in ports:adb('reverse','--remove',f'tcp:{port}')
  adb('shell','input','keyevent','224');adb('shell','wm','dismiss-keyguard')
