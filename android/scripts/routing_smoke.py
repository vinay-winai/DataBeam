"""Repeated normal-network Sendme uploads with route evidence, no forced local mode."""
from pathlib import Path
import base64,hashlib,json,os,subprocess,sys,time,uuid,xml.etree.ElementTree as ET
sys.stdout.reconfigure(encoding='utf8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';A='X:/asproj/platform-tools/adb.exe';APP='com.vinaywinai.databeam'
P=str(R/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe');C=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
def adb(*args,check=True):return subprocess.run([A,*args],capture_output=True,text=True,encoding='utf8',errors='replace',check=check,timeout=20).stdout.strip()
def state():
 text=adb('shell','run-as',APP,'cat','shared_prefs/transfer.xml',check=False)
 return json.loads(next((n.text for n in ET.fromstring(text) if n.get('name')=='state'),'{}')) if text else {}
initial=state()
assert initial.get('status') not in ['running','stopping'] or initial.get('id','').startswith('route-'),'A user transfer is active'
settings=adb('shell','run-as',APP,'cat','shared_prefs/settings.xml',check=False)
results=[];mib=int(os.environ.get('DATABEAM_ROUTING_MIB','64'));digest=hashlib.sha256()
for _ in range(mib):digest.update(bytes(i%251 for i in range(32768))*32)
expected=digest.hexdigest()
for mode in sys.argv[1:] or ['Sendme','EasySendMe']*3:
 token='route-'+uuid.uuid4().hex[:8];procs=[];handles=[];ports=[]
 record=dict(id=token,mode=mode,mib=mib)
 def spawn(args,name,env=None):
  log=(W/(token+'-'+name+'.log')).open('w',encoding='utf8');handles.append(log)
  p=subprocess.Popen(args,stdout=log,stderr=log,env=env);procs.append(p);return p
 try:
  public=os.environ.get('DATABEAM_PUBLIC_HANDOFF')=='1'
  code=token
  while public and hashlib.sha256(code.encode()).digest()[-1]%4!=0:code+='x'
  cfg=dict(id=token,mode=mode,action='send',fixture=True,fixtureMiB=mib,code=code,relay='' if public else '127.0.0.1:39009')
  if mode=='EasySendMe' and not public:
   spawn([C,'relay','--host','127.0.0.1','--ports','39009,39010,39011,39012,39013'],'relay')
   for port in range(39009,39014):adb('reverse',f'tcp:{port}',f'tcp:{port}');ports.append(port)
   time.sleep(.3)
  encoded=base64.b64encode(json.dumps(cfg).encode()).decode()
  spawn([A,'shell','am','instrument','-w','-e','config',encoded,APP+'.test/'+APP+'.SmokeInstrumentation'],'phone')
  until=time.monotonic()+40
  while time.monotonic()<until:
   s=state()
   if s.get('id')==token and s.get('ticket'):break
   time.sleep(.2)
  else:raise AssertionError('No sharing ticket')
  if mode=='EasySendMe':
   handoff=subprocess.run([C,'--yes','--ignore-stdin','--relay','1.getcroc.com:9009' if public else cfg['relay']],env=dict(os.environ,CROC_SECRET=code,CROC_NO_UPDATE_CHECK='1'),capture_output=True,text=True,encoding='utf8',timeout=30)
   assert s['ticket'] in handoff.stdout,'Ticket exchange failed'
  dest=W/(token+'-received');dest.mkdir()
  receiver=spawn([P,'receive',s['ticket'],str(dest),str(W/(token+'-cache'))],'receiver',dict(os.environ,RUST_LOG='iroh=debug'))
  started=time.monotonic();until=started+90;relay_since=None;routes=[];require_direct=os.environ.get('DATABEAM_REQUIRE_DIRECT')=='1'
  while time.monotonic()<until and receiver.poll() is None:
   s=state();values=list(s.get('senderRoutes',{}).values())
   routes.extend(v for v in values if v not in routes)
   if 'direct' in values:relay_since=None
   elif 'relay' in values and s.get('done',0)>0:
    if relay_since is None:relay_since=time.monotonic()
    if require_direct and time.monotonic()-relay_since>15:break
   time.sleep(.25)
  s=state();record.update(elapsed_seconds=round(time.monotonic()-started,2),routes=routes,bytes=s.get('done',0))
  if receiver.poll()==0:
   for line in (W/(token+'-receiver.log')).read_text(encoding='utf8',errors='replace').splitlines():
    if line.startswith('receive-route ') and line.split(' ',1)[1] not in routes:routes.append(line.split(' ',1)[1])
   file=next(dest.rglob('smoke-fixture.bin'))
   with file.open('rb') as stream:assert hashlib.file_digest(stream,'sha256').hexdigest()==expected
   record['status']='PASS';record['sha256']=expected
  else:record['status']='RELAY_FALLBACK' if relay_since else 'TIMEOUT'
  (W/(token+'-state.json')).write_text(json.dumps(s,indent=2),encoding='utf8')
  (W/(token+'-verified.json')).write_text(json.dumps(record,indent=2),encoding='utf8')
  results.append(record);print(json.dumps(record),flush=True)
 finally:
  adb('shell','am','force-stop',APP,check=False)
  if state().get('id')==token:adb('shell','run-as',APP,'rm','shared_prefs/transfer.xml',check=False)
  for p in reversed(procs):
   if p.poll() is None:p.terminate()
  for h in handles:h.close()
  for port in ports:adb('reverse','--remove',f'tcp:{port}',check=False)
print('SUMMARY',json.dumps(results),flush=True)
if settings:
 subprocess.run([A,'shell','run-as',APP,'tee','shared_prefs/settings.xml'],input=settings,capture_output=True,text=True,encoding='utf8',check=True)
if any(r['status']!='PASS' or (os.environ.get('DATABEAM_REQUIRE_DIRECT')=='1' and 'direct' not in r['routes']) for r in results):sys.exit(1)
