"""Large folder/multiple-selection regression with empty dirs and zero-byte files."""
from pathlib import Path
import subprocess,os,time,sys,json,base64,uuid,hashlib,xml.etree.ElementTree as ET,shlex
from urllib.parse import unquote,urlparse
sys.stdout.reconfigure(encoding='utf8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';A='X:/asproj/platform-tools/adb.exe';APP='com.vinaywinai.databeam'
C=str(R/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe');P=str(R/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe')
block=bytes(i%251 for i in range(32768))*32;large=hashlib.sha256()
for _ in range(128):large.update(block)
expected={'large.bin':large.hexdigest(),'zero.bin':hashlib.sha256(b'').hexdigest(),'databeam_hash.txt':hashlib.sha256(b'user-provided file').hexdigest()}
expected.update({'small-'+str(i)+'.bin':hashlib.sha256(bytes([7])*4096).hexdigest() for i in range(16)})
def adb(*args):return subprocess.check_output([A,*args],text=True,encoding='utf8').strip()
def state():
 raw=adb('shell','run-as',APP,'cat','shared_prefs/transfer.xml')
 return json.loads(next(n.text for n in ET.fromstring(raw) if n.get('name')=='state')) if raw else {}
for arg in sys.argv[1:] or [m+':'+d for m in ['Croc','Sendme','EasySendMe'] for d in ['receive','send']]:
 mode,direction=arg.split(':');token='folder-'+uuid.uuid4().hex[:8];procs=[];handles=[];ports=[]
 def spawn(cmd,name,pipe=False,env=None):
  log=(W/(token+'-'+name+'.log')).open('w',encoding='utf8');handles.append(log)
  p=subprocess.Popen(cmd,stdout=subprocess.PIPE if pipe else log,stderr=log,text=True,encoding='utf8',env=env);procs.append(p);return p
 try:
  cfg=dict(id=token,mode=mode,action=direction,code=token,relay='127.0.0.1:39009',testTimeoutSeconds=300)
  if mode!='Sendme':
   spawn([C,'relay','--host','127.0.0.1','--ports','39009,39010,39011,39012,39013'],'relay')
   for port in range(39009,39014):adb('reverse',f'tcp:{port}',f'tcp:{port}');ports.append(port)
   time.sleep(.5)
  if direction=='receive':
   folder=W/(token+'-source');(folder/'nested/empty directory').mkdir(parents=True)
   (folder/'zero.bin').write_bytes(b'');(folder/'databeam_hash.txt').write_bytes(b'user-provided file')
   (folder/'tiny').mkdir()
   for i in range(16):(folder/'tiny'/('small-'+str(i)+'.bin')).write_bytes(bytes([7])*4096)
   with (folder/'nested/large.bin').open('wb') as f:
    for _ in range(128):f.write(block)
   if mode=='Croc':spawn([C,'--yes','--ignore-stdin','--relay',cfg['relay'],'send','--',str(folder)],'sender',env=dict(os.environ,CROC_SECRET=token))
   else:
    peer=spawn([P,'send',str(folder),str(W/(token+'-cache'))],'sender',True)
    ticket=peer.stdout.readline().strip().removeprefix('TICKET=');assert ticket.startswith('blob'),ticket
    if mode=='Sendme':cfg['ticket']=ticket
    else:spawn([C,'--yes','--ignore-stdin','--relay',cfg['relay'],'send','--text',ticket],'handoff',env=dict(os.environ,CROC_SECRET=token))
  else:cfg['folderFixture']=True
  encoded=base64.b64encode(json.dumps(cfg).encode()).decode()
  phone=spawn([A,'shell','am','instrument','-w','-e','config',encoded,APP+'.test/'+APP+'.SmokeInstrumentation'],'phone')
  if direction=='send':
   deadline=time.monotonic()+45
   while time.monotonic()<deadline:
    s=state()
    if s.get('id')==token and (mode=='Croc' or s.get('ticket')):break
    time.sleep(.2)
   if mode=='EasySendMe':
    result=subprocess.run([C,'--yes','--ignore-stdin','--relay',cfg['relay']],env=dict(os.environ,CROC_SECRET=token),capture_output=True,text=True,encoding='utf8',timeout=45)
    assert s['ticket'] in result.stdout,'Ticket exchange failed'
   dest=W/(token+'-received');dest.mkdir()
   cmd=[C,'--yes','--ignore-stdin','--relay',cfg['relay'],'--out',str(dest)] if mode=='Croc' else [P,'receive',s['ticket'],str(dest),str(W/(token+'-receive-cache'))]
   receiver=spawn(cmd,'receiver',env=dict(os.environ,CROC_SECRET=token));receiver.wait(timeout=310)
   assert receiver.returncode==0,'Desktop receiver failed'
   for name,digest in expected.items():
    file=next(dest.rglob(name))
    with file.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==digest,name
   assert any(d.is_dir() for d in dest.rglob('empty directory')),'Empty directory was lost'
   extra=next(dest.rglob('smoke-extra-*.txt'));assert extra.read_text()=='second selection'
   if mode=='Croc':
    final=state();files=[f for f in dest.rglob('*') if f.is_file()]
    assert final.get('overallProgress'),'No Croc selection-wide telemetry'
    assert final['done']==final['total']==sum(f.stat().st_size for f in files),'Croc byte totals did not cover the whole selection'
    assert final['filesDone']==final['filesTotal']==len(files),'Croc file count did not cover zero-byte/multiple selections'
  else:
   phone.wait(timeout=330)
   log=(W/(token+'-phone.log')).read_text(encoding='utf8')
   assert 'INSTRUMENTATION_CODE: 0' in log,log[-3500:]
   for name,digest in expected.items():assert 'download_hash_'+name+'='+digest in log,name
   s=state();dirs=s.get('publishedDirectories',{});assert dirs,'No empty directories published'
   if mode=='Croc':
    assert s.get('overallProgress') and s['done']==s['total']==128*1024*1024+len(b'user-provided file')+16*4096
    assert s['filesDone']==s['filesTotal']==19
   for relative,uri in dirs.items():
    assert relative.endswith('empty directory'),relative
    path=unquote(urlparse(uri).path);assert adb('shell',shlex.join(['test','-d',path]))==''
  record=dict(mode=mode,direction=direction,status='PASS',large_mib=128,empty_directories=True,zero_byte_file=True,user_signature_filename=True,multiple_selections=direction=='send',hashes=expected)
  (W/(token+'-verified.json')).write_text(json.dumps(record,indent=2),encoding='utf8')
  print('PASS',arg,'128 MiB folder, empty directory, zero-byte and sentinel-named files'+('; multiple selected paths' if direction=='send' else '; automatic Downloads checksums'),flush=True)
 finally:
  subprocess.run([A,'shell','am','force-stop',APP],capture_output=True)
  for p in reversed(procs):
   if p.poll() is None:p.terminate()
  for h in handles:h.close()
  for port in ports:subprocess.run([A,'reverse','--remove',f'tcp:{port}'],capture_output=True)
