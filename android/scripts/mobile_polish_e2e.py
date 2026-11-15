"""Bounded Android UI and exact-text interoperability checks; generated data only."""
from pathlib import Path
import argparse, hashlib, json, os, shutil, socket, subprocess, time, uuid
import xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[2]
ADB='X:/asproj/platform-tools/adb.exe'
APP='com.vinaywinai.databeam'
RUNNER=APP+'.test/'+APP+'.SmokeInstrumentation'
CROC=ROOT/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe'
PEER=ROOT/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe'

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--case',action='append');parser.add_argument('--file-only',action='store_true');parser.add_argument('--normal-network',action='store_true');parser.add_argument('--relay-index',type=int,choices=(1,2,3,4));parser.add_argument('--croc-exe',type=Path);parser.add_argument('--sender-first',action='store_true');args=parser.parse_args()
    croc=args.croc_exe or CROC
    phone_relay=os.environ.get('DATABEAM_TEST_PHONE_RELAY')
    work=ROOT/'android/.tools/smoke/mobile-polish'/str(uuid.uuid4());work.mkdir(parents=True)
    children=[];handles=[];runs=[];snap={};results=[]
    normal_network=args.normal_network
    relay_index=args.relay_index
    def adb(*args,data=None,check=True):
        p=subprocess.run([ADB,*map(str,args)],input=data,capture_output=True,timeout=30)
        if check and p.returncode:raise RuntimeError((p.stdout+p.stderr).decode(errors='replace'))
        return p.stdout
    for name in ('transfer.xml','settings.xml','permissions.xml','troubleshoot-news.xml'):
        p=subprocess.run([ADB,'exec-out','run-as',APP,'cat','shared_prefs/'+name],capture_output=True,timeout=30)
        if p.returncode or not p.stdout.lstrip().startswith(b'<?xml'):
            if b'No such file or directory' not in p.stdout+p.stderr:raise RuntimeError((p.stdout+p.stderr).decode(errors='replace'))
            snap[name]=None
        else:ET.fromstring(p.stdout);snap[name]=p.stdout;(work/name).write_bytes(p.stdout)
    if snap['transfer.xml']:
        node=ET.fromstring(snap['transfer.xml']).find("string[@name='state']")
        assert node is None or json.loads(node.text).get('status') not in ('running','stopping'),'A user transfer is active'
    def spawn(command,label,env=None,stdin=None):
        out=(work/(label+'.out')).open('wb');err=(work/(label+'.err')).open('wb');handles.extend((out,err))
        p=subprocess.Popen(list(map(str,command)),stdin=stdin,stdout=out,stderr=err,env=env,creationflags=0x08000000)
        children.append(p);return p
    def instrument(label,**args):
        run=str(uuid.uuid4())
        while normal_network and relay_index and hashlib.sha256(('polish-'+run[:8]).encode()).digest()[-1]%4+1!=relay_index:run=str(uuid.uuid4())
        runs.append(run)
        before=args.pop('_prestart',None)
        if before:before('polish-'+run[:8])
        command=[ADB,'shell','am','instrument','-w','-e','run',run,'-e','relay',phone_relay or ("''" if normal_network else '127.0.0.1:19009')]
        for key,value in args.items():command+=['-e',key,str(value)]
        return run,spawn(command+[RUNNER],label)
    def install(package,path,test=False):
        location=adb('shell','pm','path',package,check=False).decode().strip().removeprefix('package:')
        if location.startswith('/data/app/') and location.endswith('/base.apk'):
            checksum=adb('shell','sha256sum',location,check=False).decode().split()
            if checksum and checksum[0]==hashlib.sha256(path.read_bytes()).hexdigest():return
        adb('install','-r',*(['-t'] if test else []),path)
    def passed(phone,label):
        phone.wait(timeout=105)
        output=(work/(label+'.out')).read_text(encoding='utf8',errors='replace')
        assert 'INSTRUMENTATION_RESULT: result=PASS' in output and 'shortMsg=' not in output,output[-2500:]
    try:
        print('Artifacts:',work,flush=True)
        spawn([ADB,'logcat','-v','threadtime','-T','1','AndroidRuntime:E','libc:F','DEBUG:F','*:S'],'live-crash')
        install(APP,ROOT/'android/app/build/outputs/apk/debug/app-debug.apk')
        install(APP+'.test',ROOT/'android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk',True)
        relay_binary=work/'croc-relay.exe';shutil.copyfile(croc,relay_binary)
        relay=spawn([relay_binary,'relay','--host','127.0.0.1','--ports','19009,19010'],'relay')
        time.sleep(1);assert relay.poll() is None
        for port in (19009,19010):adb('reverse',f'tcp:{port}',f'tcp:{port}')
        run,phone=instrument('cards',polish_test='true');passed(phone,'cards')
        results.append(dict(case='cards-controls-colors-folder-intent',status='PASS'));print('PASS cards, controls, colors and folder intent',flush=True)
        fixture=work/('polish-'+uuid.uuid4().hex+'.bin');fixture.write_bytes(bytes(range(256))*4096)
        peer_log=(work/'ticket-peer.err').open('wb');handles.append(peer_log)
        peer=subprocess.Popen([str(PEER),'send',str(fixture),str(work/'ticket-cache')],stdout=subprocess.PIPE,stderr=peer_log,creationflags=0x08000000)
        children.append(peer);ticket=peer.stdout.readline().decode().strip().removeprefix('TICKET=');assert ticket.startswith('blob')
        cases=[] if args.file_only else [tuple(case.split(':')) for case in args.case] if args.case else [('send','ticket'),('send','multiline'),('send','large'),('receive','multiline'),('receive','large')]
        for number,(direction,kind) in enumerate(cases):
            label=direction+'-'+kind+'-'+str(number);prestarted=[]
            def start_sender(code):
                assert direction=='receive' and kind=='ticket'
                relay=str(hashlib.sha256(code.encode()).digest()[-1]%4+1)+'.getcroc.com'
                relay=socket.getaddrinfo(relay,9009,socket.AF_INET,socket.SOCK_STREAM)[0][4][0]+':9009'
                prestarted.append(spawn([croc,'--yes','--ignore-stdin','--relay',relay,'send','--text',ticket],label+'-peer',dict(os.environ,CROC_SECRET=code,CROC_NO_UPDATE_CHECK='1')))
                time.sleep(2)
            run,phone=instrument(label,polish_text=kind,direction=direction,ticket=ticket,_prestart=start_sender if args.sender_first else None)
            end=time.monotonic()+35;record=None
            while time.monotonic()<end:
                p=subprocess.run([ADB,'exec-out','run-as',APP,'cat',f'files/polish-{run}.json'],capture_output=True,timeout=10)
                if p.returncode==0 and p.stdout.lstrip().startswith(b'{'):record=json.loads(p.stdout);break
                if b'No such file or directory' not in p.stdout+p.stderr:raise RuntimeError((p.stdout+p.stderr).decode(errors='replace'))
                if phone.poll() is not None:break
                time.sleep(.2)
            assert record is not None,(work/(label+'.out')).read_text(encoding='utf8',errors='replace')[-2500:]
            expected=record['expected'].encode('utf8');env=dict(os.environ,CROC_SECRET=record['code'],CROC_NO_UPDATE_CHECK='1')
            destination=work/label;destination.mkdir()
            if direction=='send':
                pc=spawn([croc,'--yes','--ignore-stdin','--ip','127.0.0.1:19009','--relay','127.0.0.1:19009','--out',destination],label+'-peer',env)
                pc.wait(timeout=65);assert pc.returncode==0,(work/(label+'-peer.err')).read_text(errors='replace')[-1500:]
                assert (work/(label+'-peer.out')).read_bytes()==expected,'Croc changed outgoing text'
            else:
                if prestarted:
                    pc=prestarted[0];pc.wait(timeout=65)
                elif args.normal_network:
                    relay=str(hashlib.sha256(record['code'].encode()).digest()[-1]%4+1)+'.getcroc.com:9009'
                    relay=socket.getaddrinfo(relay.split(':')[0],9009,socket.AF_INET,socket.SOCK_STREAM)[0][4][0]+':9009'
                    pc=spawn([croc,'--yes','--ignore-stdin','--relay',relay,'send','--text',record['expected']],label+'-peer',env)
                    pc.wait(timeout=65)
                else:
                    pc=spawn([croc,'--yes','--relay','127.0.0.1:19009','send','--no-local','--text','-'],label+'-peer',env,subprocess.PIPE)
                    pc.communicate(input=expected,timeout=65)
                assert pc.returncode==0,(work/(label+'-peer.out')).read_text(errors='replace')[-1500:]+(work/(label+'-peer.err')).read_text(errors='replace')[-1500:]
            passed(phone,label);results.append(dict(case=label,bytes=len(expected),status='PASS'));print('PASS',label,len(expected),'bytes',flush=True)
        run,phone=instrument('receive-file',polish_file='true',ticket=ticket,expectedHash=hashlib.sha256(fixture.read_bytes()).hexdigest())
        passed(phone,'receive-file');results.append(dict(case='receive-file',bytes=fixture.stat().st_size,status='PASS'))
        print('PASS receive-file SHA-256, automatic save/cleanup and Open folder',flush=True)
        (work/'results.json').write_text(json.dumps(results,indent=2))
    finally:
        # The initial inactive-state check and scoped runs make this our test app session.
        adb('shell','am','force-stop',APP,check=False)
        for p in children:
            if p.poll() is None:p.terminate()
        for p in children:
            try:p.wait(timeout=10)
            except subprocess.TimeoutExpired:p.kill();p.wait(timeout=10)
        for run in runs:
            uuid.UUID(run);adb('shell','run-as',APP,'rm','-f',f'files/polish-{run}.json',check=False)
        for name,data in snap.items():
            if data is None:adb('shell','run-as',APP,'rm','-f','shared_prefs/'+name)
            else:adb('shell','run-as',APP,'tee','shared_prefs/'+name,data=data)
            adb('shell','run-as',APP,'rm','-f','shared_prefs/'+name+'.bak',check=False)
        for port in (19009,19010):adb('reverse','--remove',f'tcp:{port}',check=False)
        for handle in handles:handle.close()
        print('Original preferences restored; test relay and reverse mappings removed.',flush=True)

if __name__=='__main__':main()
