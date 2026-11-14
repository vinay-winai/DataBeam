import sys
sys.stdout.reconfigure(encoding="utf-8")
"""Explicit on-device interoperability checks using only generated fixtures."""
import argparse,base64,hashlib,json,os,re,subprocess,threading,time,uuid
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
WORK=ROOT/'android/.tools/smoke'
ADB='X:/asproj/platform-tools/adb.exe'
CROC=str(ROOT/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe')
PEER=str(ROOT/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe')
RUNNER='com.vinaywinai.databeam.test/com.vinaywinai.databeam.SmokeInstrumentation'
def instrument(c):
 payload=base64.b64encode(json.dumps(c).encode()).decode()
 return [ADB,'shell','am','instrument','-w','-e','config',payload,RUNNER]
def main():
 parser=argparse.ArgumentParser();parser.add_argument('mode',choices=['Croc','Sendme','EasySendMe']);parser.add_argument('direction',choices=['receive','send']);args=parser.parse_args()
 WORK.mkdir(parents=True,exist_ok=True)
 fixture=WORK/'fixture.bin';fixture.write_bytes(bytes(i%251 for i in range(32768))*32)
 digest=hashlib.sha256(fixture.read_bytes()).hexdigest();token=uuid.uuid4().hex[:10]
 cfg=dict(id='smoke-'+token,mode=args.mode,action=args.direction,code='dbtest-'+token,relay='127.0.0.1:39009')
 procs=[];files=[];reverse=[]
 def spawn(cmd,name,env=None):
  f=(WORK/(token+'-'+name+'.log')).open('w');files.append(f)
  p=subprocess.Popen(cmd,stdout=f,stderr=f,env=env);procs.append(p);return p
 try:
  if args.mode!='Sendme':
   spawn([CROC,'relay','--host','127.0.0.1','--ports','39009,39010,39011,39012,39013'],'relay')
   for port in range(39009,39014):
    subprocess.run([ADB,'reverse',f'tcp:{port}',f'tcp:{port}'],check=True,capture_output=True);reverse.append(port)
   time.sleep(1)
  if args.direction=='receive':
   if args.mode=='Croc':
    spawn([CROC,'--yes','--ignore-stdin','--relay',cfg['relay'],'send','--',str(fixture)],'sender',dict(os.environ,CROC_SECRET=cfg['code']))
   else:
    f=(WORK/(token+'-peer.log')).open('w');files.append(f)
    peer=subprocess.Popen([PEER,'send',str(fixture),str(WORK/(token+'-sender-cache'))],stdout=subprocess.PIPE,stderr=f,text=True);procs.append(peer)
    ticket=peer.stdout.readline().strip().removeprefix('TICKET=');assert ticket.startswith('blob'),ticket
    if args.mode=='Sendme':cfg['ticket']=ticket
    else:spawn([CROC,'--yes','--ignore-stdin','--relay',cfg['relay'],'send','--text',ticket],'handoff',dict(os.environ,CROC_SECRET=cfg['code']))
   result=subprocess.run(instrument(cfg),capture_output=True,text=True,encoding="utf-8",errors="replace",timeout=145)
   output=result.stdout+result.stderr;(WORK/(token+'-android.log')).write_text(output)
   print(output[-9000:]);assert digest in output,'Received content hash did not match'
  else:
   cfg['fixture']=True
   if args.mode=='Croc':
    dest=WORK/(token+'-receive');dest.mkdir()
    receiver=spawn([CROC,'--yes','--ignore-stdin','--relay',cfg['relay'],'--out',str(dest)],'receiver',dict(os.environ,CROC_SECRET=cfg['code']))
    phone=subprocess.Popen(instrument(cfg),stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True);procs.append(phone)
    text=phone.communicate(timeout=145)[0];(WORK/(token+'-android.log')).write_text(text);print(text[-8000:]);receiver.wait(timeout=15)
    received=list(dest.rglob('smoke-fixture.bin'));assert received and hashlib.sha256(received[0].read_bytes()).hexdigest()==digest
   else:
    phone=subprocess.Popen(instrument(cfg),stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True);procs.append(phone)
    lines=[];ticket=None
    for line in phone.stdout:
     lines.append(line)
     if 'ticket=' in line:ticket=line.split('ticket=',1)[1].strip();break
     if phone.poll() is not None:break
    print('Phone ticket obtained:',bool(ticket),flush=True);assert ticket,'No ticket: '+''.join(lines)
    if args.mode=='EasySendMe':
     handoff=subprocess.run([CROC,'--yes','--ignore-stdin','--relay',cfg['relay']],env=dict(os.environ,CROC_SECRET=cfg['code']),capture_output=True,text=True,encoding="utf-8",errors="replace",timeout=60)
     (WORK/(token+'-handoff.log')).write_text(handoff.stdout+handoff.stderr)
     match=re.search(r'\bblob[a-z2-7]+\b',handoff.stdout)
     assert handoff.returncode==0 and match,'Croc ticket exchange failed: '+handoff.stderr
     assert match.group()==ticket,'Ticket received through Croc differs from native share'
    dest=WORK/(token+'-receive')
    received=subprocess.run([PEER,'receive',ticket,str(dest),str(WORK/(token+'-cache'))],capture_output=True,text=True,encoding="utf-8",errors="replace",timeout=90)
    print(received.stdout,received.stderr);assert received.returncode==0
    payload=list(dest.rglob('smoke-fixture.bin'));assert payload and hashlib.sha256(payload[0].read_bytes()).hexdigest()==digest
  print('PASS',args.mode,args.direction,'SHA256',digest,flush=True)
 finally:
  subprocess.run([ADB,"shell","am","force-stop","com.vinaywinai.databeam"],capture_output=True)
  for p in reversed(procs):
   if p.poll() is None:p.terminate()
  for p in procs:
   try:p.wait(timeout=10)
   except subprocess.TimeoutExpired:p.kill()
  for f in files:f.close()
  for port in reverse:subprocess.run([ADB,'reverse','--remove',f'tcp:{port}'],capture_output=True)
if __name__=='__main__':main()
