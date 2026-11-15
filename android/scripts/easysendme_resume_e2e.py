"""Generated-data EasySendMe UI resume regression; preserves existing phone state."""
from pathlib import Path
import argparse,hashlib,json,os,re,shutil,subprocess,time,uuid,xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[2]
ADB='X:/asproj/platform-tools/adb.exe'
APP='com.vinaywinai.databeam'
PEER=ROOT/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe'
CROC=ROOT/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe'

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--desktop-sender',action='store_true');parser.add_argument('--same-code',action='store_true');parser.add_argument('--mode',choices=('Sendme','EasySendMe'),default='EasySendMe');args=parser.parse_args()
    assert args.mode!='Sendme' or args.desktop_sender,'Sendme mode uses the bounded normal-desktop scenario'
    peer=ROOT/'target/debug/examples/android_sender.exe' if args.desktop_sender else PEER
    run=str(uuid.uuid4());work=ROOT/'android/.tools/smoke/easy-resume'/run;work.mkdir(parents=True)
    print('Artifacts:',work,flush=True)
    children=[];handles=[];snap={};session='';sessions=set();record={}
    def adb(*args,data=None,check=True):
        p=subprocess.run([ADB,*map(str,args)],input=data,capture_output=True,timeout=35)
        if check and p.returncode:raise RuntimeError((p.stdout+p.stderr).decode(errors='replace'))
        return p
    for name in ('transfer.xml','settings.xml','permissions.xml','troubleshoot-news.xml'):
        p=adb('exec-out','run-as',APP,'cat','shared_prefs/'+name,check=False)
        if p.returncode:
            if b'No such file or directory' not in p.stdout+p.stderr:raise RuntimeError((p.stdout+p.stderr).decode(errors='replace'))
            snap[name]=None
        else:ET.fromstring(p.stdout);snap[name]=p.stdout;(work/name).write_bytes(p.stdout)
    if snap['transfer.xml']:
        node=ET.fromstring(snap['transfer.xml']).find("string[@name='state']")
        assert node is None or json.loads(node.text).get('status') not in ('running','stopping'),'A user transfer is active'
    def spawn(cmd,label,env=None):
        out=(work/(label+'.out')).open('wb');err=(work/(label+'.err')).open('wb');handles.extend((out,err))
        p=subprocess.Popen(list(map(str,cmd)),stdout=out,stderr=err,env=env,cwd=work,creationflags=0x08000000);children.append(p);return p
    def stop(p):
        if p.poll() is None:
            p.terminate()
            try:p.wait(timeout=10)
            except subprocess.TimeoutExpired:p.kill();p.wait(timeout=10)
    def sender(label):
        p=spawn([peer,'send',fixture,work/(label+'-cache')],label)
        end=time.monotonic()+25
        while time.monotonic()<end:
            match=re.search(r'^TICKET=(blob[a-z2-7]+)',(work/(label+'.out')).read_text(errors='replace'),re.M)
            if match:return p,match.group(1)
            assert p.poll() is None,'Native sender stopped';time.sleep(.1)
        raise AssertionError('No sender ticket')
    def handoff(code,ticket,label):
        return spawn([CROC,'--yes','--relay','127.0.0.1:19009','send','--no-local','--text',ticket],label,dict(os.environ,CROC_SECRET=code,CROC_NO_UPDATE_CHECK='1'))
    def marker():
        p=adb('exec-out','run-as',APP,'cat',f'files/easy-resume-{run}.json',check=False)
        if p.returncode:
            if b'No such file or directory' in p.stdout+p.stderr:return {}
            raise RuntimeError((p.stdout+p.stderr).decode(errors='replace'))
        try:return json.loads(p.stdout)
        except json.JSONDecodeError:return {} # Atomicity is not needed for this test-only marker.
    def install(path,test=False):
        package=APP+'.test' if test else APP
        location=adb('shell','pm','path',package,check=False).stdout.decode().strip().removeprefix('package:')
        if location.startswith('/data/app/') and location.endswith('/base.apk'):
            installed=adb('shell','sha256sum',location,check=False).stdout.decode().split()
            if installed and installed[0]==hashlib.sha256(path.read_bytes()).hexdigest():return
        adb('install','-r',*(['-t'] if test else []),path)
    try:
        install(ROOT/'android/app/build/outputs/apk/debug/app-debug.apk')
        install(ROOT/'android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk',True)
        relay_binary=work/'relay.exe';shutil.copyfile(CROC,relay_binary)
        relay=spawn([relay_binary,'relay','--host','127.0.0.1','--ports','19009,19010'],'relay');time.sleep(1);assert relay.poll() is None
        for port in (19009,19010):adb('reverse',f'tcp:{port}',f'tcp:{port}')
        fixture=work/('easy-resume-'+run[:8]+'.bin');block=bytes(i%251 for i in range(1024*1024));digest=hashlib.sha256()
        with fixture.open('wb') as f:
            for _ in range(256):f.write(block);digest.update(block)
        first,ticket=sender('sender-a');code='easy-'+run[:8]+'-a';exchanges=[]
        if args.mode=='EasySendMe':exchanges.append(handoff(code,ticket,'handoff-a'))
        phone=spawn([ADB,'shell','am','instrument','-w','-e','easy_resume_test','true','-e','mode',args.mode,'-e','ticket',ticket,'-e','one_shot',str(args.desktop_sender).lower(),'-e','run',run,'-e','code',code,'-e','sha256',digest.hexdigest(),APP+'.test/'+APP+'.SmokeInstrumentation'],'phone')
        deadline=time.monotonic()+200;restarted=False;fresh_started=False
        while phone.poll() is None and time.monotonic()<deadline:
            record=marker();session=record.get('session') or record.get('id') or session
            sessions.update(record.get('sessions',[]))
            if record.get('stage')=='restart-sender' and not restarted:
                if args.desktop_sender:
                    try:first.wait(timeout=8)
                    except subprocess.TimeoutExpired:pass
                    print('Normal desktop sender after receiver pause:',('ended' if first.poll() is not None else 'still sharing'),flush=True)
                stop(first);second,new_ticket=sender('sender-b');assert new_ticket!=ticket
                new_code=code if args.same_code else 'easy-'+run[:8]+'-b'
                if args.mode=='EasySendMe':exchanges.append(handoff(new_code,new_ticket,'handoff-b'))
                ready=json.dumps(dict(code=new_code,ticket=new_ticket)).encode()
                adb('shell','run-as',APP,'tee',f'files/easy-ready-{run}.json',data=ready)
                restarted=True;print('Restarted sender with same content and new ticket/code',flush=True)
            if record.get('stage')=='fresh-receive' and not fresh_started:
                fresh_code='easy-'+run[:8]+'-c';exchanges.append(handoff(fresh_code,new_ticket,'handoff-c'))
                adb('shell','run-as',APP,'tee',f'files/easy-ready-{run}.json',data=json.dumps(dict(code=fresh_code)).encode())
                fresh_started=True;print('Started a fresh code handoff for automatic cross-card cache reuse',flush=True)
            time.sleep(.3)
        assert phone.poll() is not None,'Phone regression timed out'
        record=marker();session=record.get('id') or session
        sessions.update(record.get('sessions',[]))
        (work/'final-state.json').write_text(json.dumps(record,indent=2))
        output=(work/'phone.out').read_text(errors='replace');print(output[-4500:],flush=True)
        assert restarted and 'INSTRUMENTATION_RESULT: result=PASS' in output and 'INSTRUMENTATION_CODE: 0' in output,output[-4500:]
        for exchange in exchanges:exchange.wait(timeout=10);assert exchange.returncode==0,'Croc handoff failed'
        if args.desktop_sender:
            second.wait(timeout=10);assert second.returncode==0,'Normal desktop sender failed after receive completion'
        (work/'result.json').write_text(json.dumps(dict(status='PASS',bytes=fixture.stat().st_size,sha256=digest.hexdigest(),session=session)))
    finally:
        adb('shell','am','force-stop',APP,check=False)
        for p in reversed(children):stop(p)
        if not record:record=marker()
        if record.get('id')==session:
            for uri in record.get('published',{}).values():
                if re.fullmatch(r'content://media/(external|external_primary)/downloads/\d+',uri):adb('shell','content','delete','--uri',uri,check=False)
        if session:sessions.add(session)
        for session_id in sessions:
            uuid.UUID(session_id)
            for parent in ('blobs','received','outgoing'):
                base=adb('shell','run-as',APP,'readlink','-f','files/'+parent).stdout.decode().strip()
                target=adb('shell','run-as',APP,'readlink','-f',f'files/{parent}/{session_id}',check=False).stdout.decode().strip()
                if target:
                    assert target==base+'/'+session_id
                    adb('shell','run-as',APP,'rm','-rf',f'files/{parent}/{session_id}')
            adb('shell','run-as',APP,'rm','-f',f'files/transfer-records/{session_id}.json',f'files/transfer-records/{session_id}.tmp')
        adb('shell','run-as',APP,'rm','-f',f'files/easy-resume-{run}.json',f'files/easy-ready-{run}.json')
        for name,data in snap.items():
            if data is None:adb('shell','run-as',APP,'rm','-f','shared_prefs/'+name)
            else:adb('shell','run-as',APP,'tee','shared_prefs/'+name,data=data)
            adb('shell','run-as',APP,'rm','-f','shared_prefs/'+name+'.bak')
        for port in (19009,19010):adb('reverse','--remove',f'tcp:{port}',check=False)
        for handle in handles:handle.close()
        print('Original phone state restored; generated private data and relay mappings removed.',flush=True)

if __name__=='__main__':main()
