"""Controlled power/network/process interruption checks; only generated fixtures.

Restores Wi-Fi, cellular-data and battery/idle overrides in finally. Native
process-kill recovery deliberately requires a manual second receive invocation.
"""
from pathlib import Path
import subprocess,sys,os,time,json,base64,uuid,hashlib,xml.etree.ElementTree as ET
sys.stdout.reconfigure(encoding='utf-8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';A='X:/asproj/platform-tools/adb.exe'
P='com.vinaywinai.databeam';RUN=P+'.test/'+P+'.SmokeInstrumentation'
C=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
PEER=str(R/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe')
fixture=W/'background-512MiB.bin'
with fixture.open('rb') as f: expected=hashlib.file_digest(f,'sha256').hexdigest()
def adb(*args,check=True):
 return subprocess.run([A,*args],capture_output=True,text=True,encoding='utf-8',errors='replace',check=check,timeout=20).stdout.strip()
def state():
 xml=adb('shell','run-as',P,'cat','shared_prefs/transfer.xml')
 if not xml: return {}
 return json.loads(next((n.text for n in ET.fromstring(xml) if n.get('name')=='state'),'{}'))
checks=sys.argv[1:] or ['doze:Croc','doze:Sendme','doze:EasySendMe','network:Sendme','kill:Sendme']
for check in checks:
 scenario,mode=check.split(':');token='recovery-'+uuid.uuid4().hex[:8];code=token
 while hashlib.sha256(code.encode()).digest()[-1]%4!=0:code+='x'
 procs=[];handles=[];mobile=adb('shell','settings','get','global','mobile_data')
 wifi=adb('shell','settings','get','global','wifi_on');record=dict(scenario=scenario,mode=mode)
 def spawn(args,name,pipe=False,env=None):
  log=(W/(token+'-'+name+'.log')).open('w',encoding='utf-8');handles.append(log)
  proc=subprocess.Popen(args,stdout=subprocess.PIPE if pipe else log,stderr=log,text=True,encoding='utf-8',env=env);procs.append(proc);return proc
 def phone(config,name):
  encoded=base64.b64encode(json.dumps(config).encode()).decode()
  return spawn([A,'shell','am','instrument','-w','-e','config',encoded,RUN],name)
 try:
  print('START',check,flush=True)
  cfg=dict(id=token,mode=mode,action='receive',code=code)
  if mode=='Croc':
   cfg['relay']='1.getcroc.com:9009'
   spawn([C,'--yes','--ignore-stdin','--relay',cfg['relay'],'--throttleUpload','10m','send','--',str(fixture)],'sender',env=dict(os.environ,CROC_SECRET=code))
  else:
   sender=spawn([PEER,'send',str(fixture),str(W/(token+'-cache'))],'sender',True)
   ticket=sender.stdout.readline().strip().removeprefix('TICKET=');assert ticket.startswith('blob'),ticket
   if mode=='Sendme':cfg['ticket']=ticket
   else:spawn([C,'--yes','--ignore-stdin','--relay','1.getcroc.com:9009','send','--text',ticket],'handoff',env=dict(os.environ,CROC_SECRET=code))
  current=phone(cfg,'phone')
  deadline=time.monotonic()+50
  while time.monotonic()<deadline:
   saved=state()
   if saved.get('id')==token:
    if saved.get('status')=='interrupted':raise AssertionError(saved.get('message'))
    if saved.get('done',0)>=8*1024*1024 or saved.get('percent',0)>=5:break
   time.sleep(.2)
  else:raise AssertionError('No payload progress before interruption')
  record['before_bytes']=saved.get('done',0);record['before_percent']=saved.get('percent',0)
  if scenario=='doze':
   adb('shell','dumpsys','battery','unplug');adb('shell','input','keyevent','3');adb('shell','input','keyevent','223')
   idle=adb('shell','dumpsys','deviceidle','force-idle');assert 'idle' in idle.lower(),idle
   assert 'mState=IDLE' in adb('shell','dumpsys','deviceidle'),'Deep idle was not entered'
   assert state().get('status')=='running','Transfer finished before forced idle'
   record['forced_idle']=True
  elif scenario=='network':
   # Avoid moving the generated 512 MiB fixture onto a metered cellular path.
   adb('shell','svc','data','disable');adb('shell','svc','wifi','disable');time.sleep(5)
   record['offline_seconds']=5;adb('shell','svc','wifi','enable')
  elif scenario=='kill':
   adb('shell','am','force-stop',P);current.wait(timeout=15)
   xml=state();assert xml['id']==token
   # TransferService.state must turn a dead running session into Interrupted.
   cfg=xml;cfg.pop('log',None);cfg['requireCachedResume']=True
   current=phone(cfg,'resume')
   record['manual_resume_after_force_stop']=True
  current.wait(timeout=140)
  saved=state()
  if scenario=='network' and saved.get('status')=='interrupted':
   cfg=saved;cfg.pop('log',None);cfg['requireCachedResume']=True
   current=phone(cfg,'resume');current.wait(timeout=140);saved=state()
   record['manual_resume']=True
  assert saved.get('status')=='complete',saved.get('message')
  logs='\n'.join((W/(token+'-'+name+'.log')).read_text(encoding='utf8') for name in ['phone','resume'] if (W/(token+'-'+name+'.log')).exists())
  assert 'download_hash_background-512MiB.bin='+expected in logs,'Published checksum not verified'
  if scenario=='kill':
   import re
   match=re.search(r'resumed_from=(\d+)',logs);assert match and int(match[1])>0,logs[-2000:]
   record['resumed_from']=int(match[1])
  record.update(status='PASS',sha256=expected,route=saved.get('route'))
  (W/(token+'-verified.json')).write_text(json.dumps(record,indent=2),encoding='utf8')
  print('PASS',json.dumps(record),flush=True)
 finally:
  adb('shell','dumpsys','deviceidle','unforce',check=False);adb('shell','dumpsys','battery','reset',check=False)
  if scenario=='network':
   adb('shell','svc','wifi','enable' if wifi!='0' else 'disable',check=False)
   adb('shell','svc','data','enable' if mobile=='1' else 'disable',check=False)
  adb('shell','svc','power','stayon','usb',check=False)
  adb('shell','am','force-stop',P,check=False)
  for proc in reversed(procs):
   if proc.poll() is None:proc.terminate()
  for handle in handles:handle.close()
