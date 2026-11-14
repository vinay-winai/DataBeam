"""Use normal Croc public rendezvous, with no ADB relay mappings, and inspect the LAN payload path."""
from pathlib import Path
import subprocess,os,base64,json,uuid,hashlib,re,sys,xml.etree.ElementTree as ET
sys.stdout.reconfigure(encoding='utf-8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';A='X:/asproj/platform-tools/adb.exe';C=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
token=uuid.uuid4().hex[:8];code='lan-'+token
mib=int(sys.argv[1]) if len(sys.argv)>1 else 1
while hashlib.sha256(code.encode()).digest()[-1]%4!=0:code+='x'
destination=W/('lan-'+token);destination.mkdir()
log_path=W/('lan-'+token+'-desktop.log');log=log_path.open('w')
receiver=subprocess.Popen([C,'--debug','--yes','--ignore-stdin','--relay','1.getcroc.com:9009','--out',str(destination)],env=dict(os.environ,CROC_SECRET=code),stdout=log,stderr=log)
try:
 cfg=dict(id='lan-'+token,mode='Croc',action='send',code=code,relay='1.getcroc.com:9009',fixture=True,fixtureMiB=mib,backgroundTest='--background' in sys.argv)
 encoded=base64.b64encode(json.dumps(cfg).encode()).decode()
 try:
  result=subprocess.run([A,'shell','am','instrument','-w','-e','config',encoded,'com.vinaywinai.databeam.test/com.vinaywinai.databeam.SmokeInstrumentation'],capture_output=True,text=True,encoding='utf-8',timeout=135)
 except subprocess.TimeoutExpired as e:
  (W/('lan-'+token+'-phone.log')).write_bytes(e.stdout or b'')
  raise
 (W/('lan-'+token+'-phone.log')).write_text(result.stdout+result.stderr,encoding='utf-8')
 receiver.wait(timeout=15);log.flush()
 text=log_path.read_text(errors='replace')
 print('\n'.join(line for line in text.splitlines() if any(key in line for key in ['Sending (','Receiving (','ipv4Add4','could not connect to','local connection','encrypted local endpoint','racing peer','local ips','received ips'])),flush=True)
 payload=next(destination.rglob('smoke-fixture.bin'))
 block=bytes(i%251 for i in range(32768))*32;expected=hashlib.sha256()
 for _ in range(mib):expected.update(block)
 with payload.open('rb') as file:assert hashlib.file_digest(file,'sha256').hexdigest()==expected.hexdigest()
 assert 'INSTRUMENTATION_CODE: 0' in result.stdout,result.stdout[-2000:]
 if '--allow-relay' not in sys.argv:assert '"route":"direct"' in result.stdout,'Normal LAN test did not select Direct'
 if cfg['backgroundTest']:assert 'screen_off=true' in result.stdout
 print('PASS',mib,'MiB public-rendezvous phone-to-PC Croc transfer; payload hash correct; log',log_path,flush=True)
finally:
 if receiver.poll() is None:receiver.terminate()
 log.close()
 # Never leave a timed-out test looking like a user's active transfer.
 raw=subprocess.run([A,'shell','run-as','com.vinaywinai.databeam','cat','shared_prefs/transfer.xml'],capture_output=True,text=True).stdout
 if raw:
  saved=json.loads(next((n.text for n in ET.fromstring(raw) if n.get('name')=='state'),'{}'))
  if saved.get('id')=='lan-'+token:
   subprocess.run([A,'shell','am','force-stop','com.vinaywinai.databeam'],capture_output=True)
   subprocess.run([A,'shell','run-as','com.vinaywinai.databeam','rm','shared_prefs/transfer.xml'],capture_output=True)
