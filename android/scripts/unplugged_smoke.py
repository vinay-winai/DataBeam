"""Real battery-powered receive: start through the app UI, then survive USB removal."""
from pathlib import Path
import subprocess,sys,os,time,json,hashlib,uuid,re,shlex,xml.etree.ElementTree as ET,psutil
sys.stdout.reconfigure(encoding='utf-8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';A='X:/asproj/platform-tools/adb.exe';P='com.vinaywinai.databeam'
C=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe');fixture=W/'background-512MiB.bin'
token=uuid.uuid4().hex[:8];code='battery-'+token
while hashlib.sha256(code.encode()).digest()[-1]%4!=0:code+='x'
def adb(*args,check=True):return subprocess.run([A,*args],capture_output=True,text=True,encoding='utf-8',errors='replace',check=check,timeout=15).stdout
def state():
 raw=adb('shell','run-as',P,'cat','shared_prefs/transfer.xml');return json.loads(next((n.text for n in ET.fromstring(raw) if n.get('name')=='state'),'{}'))
def ui():
 adb('shell','uiautomator','dump','/sdcard/databeam-battery-ui.xml');return ET.fromstring(adb('exec-out','cat','/sdcard/databeam-battery-ui.xml'))
def click(predicate,last=False):
 nodes=[n for n in ui().iter('node') if predicate(n)]
 if not nodes:raise AssertionError('UI control not found')
 node=nodes[-1 if last else 0];x1,y1,x2,y2=map(int,re.findall(r'\d+',node.get('bounds')))
 adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2))
with fixture.open('rb') as file:expected=hashlib.file_digest(file,'sha256').hexdigest()
record=dict(code=code,expected_sha256=expected,source=str(fixture),unplugged=False)
def journal():
 (W/('battery-'+token+'-events.json')).write_text(json.dumps(record,indent=2),encoding='utf-8')
log=(W/('battery-'+token+'-sender.log')).open('w',encoding='utf-8')
sender=None;paused=False
try:
 assert state().get('status') not in ['running','stopping'],'A user transfer is active'
 sender=subprocess.Popen([C,'--yes','--ignore-stdin','--relay','1.getcroc.com:9009','--throttleUpload','10m','send','--',str(fixture)],env=dict(os.environ,CROC_SECRET=code),stdout=log,stderr=log)
 adb('shell','am','start','-n',P+'/.MainActivity')
 click(lambda n:n.get('text')=='Receive' and n.get('class')=='android.widget.Button')
 click(lambda n:n.get('class')=='android.widget.Spinner')
 click(lambda n:n.get('text')=='Croc')
 click(lambda n:n.get('hint')=='Paste a code or Sendme ticket')
 adb('shell','input','keycombination','113','29');adb('shell','input','keyevent','67')
 adb('shell','input','text',code);adb('shell','input','keyevent','4')
 entered=next(n.get('text') for n in ui().iter('node') if n.get('hint')=='Paste a code or Sendme ticket')
 assert entered==code,'UI input did not replace the previous sharing code'
 click(lambda n:n.get('text')=='Receive' and n.get('class')=='android.widget.Button',last=True)
 deadline=time.monotonic()+45
 while time.monotonic()<deadline:
  saved=state()
  if saved.get('status')=='interrupted':raise AssertionError(saved.get('message'))
  if saved.get('percent',0)>=3:break
  time.sleep(.5)
 else:raise AssertionError('No live payload progress before unplugging')
 record.update(id=saved['id'],route=saved.get('route'),percent_at_ready=saved.get('percent'))
 # Save only our notification record, never other apps' notification contents.
 notifications=adb('shell','dumpsys','notification','--noredact')
 records=re.split(r'(?=NotificationRecord\()',notifications)
 ours=next((part for part in records if 'pkg='+P+' ' in part[:400]),'')
 assert ours and ('android.progress' in ours or 'progress=' in ours),'Live progress notification missing'
 (W/('battery-'+token+'-notification.txt')).write_text(ours.split('\n\n',1)[0][:10000],encoding='utf-8')
 adb('shell','input','keyevent','3');adb('shell','input','keyevent','223')
 psutil.Process(sender.pid).suspend();paused=True
 record['sender_paused_until_usb_removed']=True
 journal()
 print('READY_TO_UNPLUG: sender paused until USB is removed; phone locked; progress notification verified.',flush=True)
 deadline=time.monotonic()+260;disconnected_at=None;completed=False
 while time.monotonic()<deadline:
  connected=adb('get-state',check=False).strip()=='device'
  if not connected and disconnected_at is None:
   disconnected_at=time.monotonic();record['unplugged']=True
   journal()
   psutil.Process(sender.pid).resume();paused=False
   print('USB_DISCONNECTED: sender resumed; continuing on battery.',flush=True)
  if sender.poll() is not None:
   assert sender.returncode==0,'Desktop sender failed; inspect sender log'
   if not completed:
    completed=True;record['sender_finished_on_battery']=not connected
    journal()
    print('TRANSFER_FINISHED: reconnect USB for saved-file verification.',flush=True)
   if connected:
    saved=state()
    if saved.get('id')!=record['id']:raise AssertionError('Unexpected active session')
    if saved.get('status')=='complete':break
    if saved.get('status')=='interrupted':raise AssertionError(saved.get('message'))
  time.sleep(1)
 else:raise AssertionError('Battery test/reconnection deadline exceeded')
 assert record['unplugged'] and record['sender_finished_on_battery'],'Cable was not disconnected until completion'
 published=saved.get('published',{});assert published,'Automatic save missing'
 for relative,uri in published.items():
  info=adb('shell',shlex.join(['content','query','--uri',uri,'--projection','_data']))
  path=info.partition('_data=')[2].strip();assert path.startswith('/storage/'),info
  actual=adb('shell',shlex.join(['sha256sum',path])).split()[0]
  assert actual==expected,'Saved download hash mismatch'
 record.update(status='PASS',saved_files=list(published),sha256=expected,route=saved.get('route'))
 (W/('battery-'+token+'-verified.json')).write_text(json.dumps(record,indent=2),encoding='utf-8')
 print('PASS physical USB unplug, locked-screen battery transfer, foreground progress notification, automatic saving and SHA256',expected,flush=True)
finally:
 if paused and sender is not None and sender.poll() is None:psutil.Process(sender.pid).resume()
 if sender is not None and sender.poll() is None:sender.terminate()
 log.close()
