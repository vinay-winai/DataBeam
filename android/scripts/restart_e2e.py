"""Sender restart -> real desktop receiver -> SHA-256 verification.

Generated fixtures only. Saves preferences on the host before installing tests,
and restores them even if Android's instrumentation process dies.
"""
from pathlib import Path
import argparse,base64,hashlib,json,os,re,shutil,subprocess,time,uuid
import xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[2]
ADB='X:/asproj/platform-tools/adb.exe'
APP='com.vinaywinai.databeam'
CROC=ROOT/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe'
PEER=ROOT/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe'
RUNNER=APP+'.test/'+APP+'.SmokeInstrumentation'

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--serial',default='RZCT9012X2V')
    parser.add_argument('--case',action='append',help='MODE:SCENARIO, e.g. Sendme:stop')
    parser.add_argument('--kind',choices=('flat','folder','provider'),help='Override fixture: references, folder paths, or private/stream/bounded provider fallbacks')
    parser.add_argument('--mib',type=int,help='Override generated payload size for a bounded network repeat')
    parser.add_argument('--work',type=Path)
    parser.add_argument('--relay',default='1.getcroc.com:9009',help='Croc rendezvous endpoint shared by phone and desktop')
    args=parser.parse_args()
    work=args.work or ROOT/'android/.tools/smoke/restart-e2e'/str(uuid.uuid4())
    work=work.resolve();work.mkdir(parents=True,exist_ok=True)
    assert work.is_relative_to(ROOT.resolve()),'Test artifacts must stay in the workspace'
    procs=[];handles=[];case_ids=[];results=[]
    def adb(*command,data=None,check=True):
        p=subprocess.run([ADB,'-s',args.serial,*map(str,command)],input=data,capture_output=True,timeout=30)
        if check and p.returncode:raise RuntimeError(p.stderr.decode(errors='replace'))
        return p.stdout
    def shell(command,**kwargs):return adb('shell',command,**kwargs)
    def state():
        xml=adb('exec-out','run-as',APP,'cat','shared_prefs/transfer.xml',check=False)
        if not xml.lstrip().startswith(b'<?xml'):return {}
        node=ET.fromstring(xml).find("string[@name='state']")
        return json.loads(node.text) if node is not None else {}
    assert state().get('status') not in ('running','stopping'),'A user transfer is active'
    snapshots={}
    for name in ('settings.xml','transfer.xml'):
        # Reuse an earlier pre-install backup when this directory was prepared already.
        backup=work/name
        if not backup.exists():backup.write_bytes(adb('exec-out','run-as',APP,'cat','shared_prefs/'+name,check=False))
        snapshots[name]=backup.read_bytes() if backup.read_bytes().lstrip().startswith(b'<?xml') else b''
    def restore():
        adb('shell','am','force-stop',APP,check=False)
        for name,data in snapshots.items():
            if data:
                adb('shell','run-as',APP,'mkdir','-p','shared_prefs')
                adb('shell','run-as',APP,'tee','shared_prefs/'+name,data=data)
            else:adb('shell','run-as',APP,'rm','-f','shared_prefs/'+name,check=False)
        adb('shell','run-as',APP,'rm','-f','shared_prefs/settings.xml.bak','shared_prefs/transfer.xml.bak',check=False)
    def spawn(command,name,env=None):
        out=(work/(name+'.out')).open('wb');err=(work/(name+'.err')).open('wb');handles.extend([out,err])
        p=subprocess.Popen(list(map(str,command)),stdout=out,stderr=err,env=env,creationflags=0x08000000 if os.name=='nt' else 0)
        procs.append(p);return p
    def terminate(p):
        if p.poll() is None:
            p.terminate()
            try:p.wait(timeout=8)
            except subprocess.TimeoutExpired:p.kill();p.wait(timeout=5)
    def phone_record(case):
        raw=adb('exec-out','run-as',APP,'cat',f'files/restart-e2e/{case}/record.json',check=False)
        try:return json.loads(raw)
        except (ValueError,UnicodeError):return {}
    def wait_record(case,phase,timeout=65):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            r=phone_record(case)
            if r.get('phase')=='failed':raise AssertionError(r.get('error'))
            if r.get('phase')==phase:
                (work/(case+'-record.json')).write_text(json.dumps(r,indent=2,ensure_ascii=False),encoding='utf8');return r
            time.sleep(.15)
        raise AssertionError('Phone did not reach '+phase+': '+str(phone_record(case).get('phase')))
    def command(case,op,sequence,**extra):
        path=f'files/restart-e2e/{case}/command'
        adb('shell','run-as',APP,'tee',path+'.tmp',data=json.dumps(dict(op=op,sequence=sequence,**extra)).encode())
        adb('shell','run-as',APP,'mv',path+'.tmp',path+'.json')
    def instrument(case,mode,scenario,kind,stage='start',mib=8):
        config=dict(run=case,mode=mode,scenario=scenario,kind=kind,stage=stage,mib=mib,relay=args.relay)
        payload=base64.b64encode(json.dumps(config).encode()).decode()
        return spawn([ADB,'-s',args.serial,'shell','am','instrument','-w','-e','restart_e2e',payload,RUNNER],case+'-phone-'+stage)
    def croc_env(code):
        return dict(os.environ,CROC_SECRET=code,CROC_NO_UPDATE_CHECK='1',DATABEAM_CROC_PROGRESS='1',CROC_CONFIG_DIR=str(work/'croc-config'))
    def ticket_for(case,mode,s,label):
        if mode=='Sendme':return s['ticket']
        if mode=='Croc':return None
        p=spawn([relay_binary,'--yes','--ignore-stdin','--relay',args.relay],case+'-'+label+'-handoff',croc_env(s['code']))
        p.wait(timeout=50)
        raw=(work/(case+'-'+label+'-handoff.out')).read_text(encoding='utf8',errors='replace')
        match=re.search(r'\bblob[a-z2-7]+\b',raw)
        assert p.returncode==0 and match,'EasySendMe short-code exchange failed'
        assert match.group()==s['ticket'],'Short code returned a stale ticket'
        return match.group()
    def receiver(case,mode,s,label,destination,cache):
        ticket=ticket_for(case,mode,s,label)
        destination.mkdir(parents=True,exist_ok=True)
        if mode=='Croc':
            cmd=[relay_binary,'--yes','--ignore-stdin','--relay',args.relay,'--out',destination]
            # Consent to resume this test-owned partial file. --yes only accepts
            # the transfer; Croc asks a separate resume question without this.
            if label=='restarted' and s.get('scenario')=='mid':cmd.append('--overwrite')
            env=croc_env(s['code'])
        else:cmd=[PEER,'receive',ticket,destination,cache];env=None
        return spawn(cmd,case+'-'+label+'-receiver',env)
    def progress(case,label,mode):
        text=(work/(case+'-'+label+'-receiver.err')).read_text(encoding='utf8',errors='replace')
        if mode=='Croc':return [json.loads(m)['done'] for m in re.findall(r'DATABEAM_TRANSFER_V1 (\{[^\r\n]+\})',text)]
        return [int(n) for n in re.findall(r'receive-progress (\d+):',text)]
    def verify(case,mode,record,destination,label):
        if record['kind']=='text':
            got=(work/(case+'-'+label+'-receiver.out')).read_text(encoding='utf8').replace('\r\n','\n').rstrip('\n')
            assert got==record['text'].rstrip('\n'),'Restarted text differs'
            assert not any(p.is_file() for p in destination.rglob('*')),'Croc text leaked a file'
            return 0
        base=destination
        if mode!='Croc':
            text=(work/(case+'-'+label+'-receiver.out')).read_text(encoding='utf8',errors='replace')
            match=re.search(r'^RECEIVED=(.+)$',text,re.M);assert match,'Native receiver did not report export location'
            base=Path(match.group(1).strip()).resolve();assert base.is_relative_to(destination.resolve())
        actual={p.relative_to(base).as_posix():p for p in base.rglob('*') if p.is_file()}
        assert set(actual)==set(record['files']),f'File layout mismatch: expected {list(record["files"])}, got {list(actual)}'
        for name,expected in record['files'].items():
            file=actual[name];assert file.stat().st_size==expected['size'],name+' size mismatch'
            with file.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==expected['sha256'],name+' SHA-256 mismatch'
        for directory in record['directories']:assert (base/directory).is_dir(),'Empty directory missing: '+directory
        return sum(x['size'] for x in record['files'].values())
    def cleanup_case(case):
        record=phone_record(case)
        if record:(work/(case+'-record.json')).write_text(json.dumps(record,indent=2,ensure_ascii=False),encoding='utf8')
        adb('shell','am','force-stop',APP,check=False)
        for uri in record.get('uris',[]):
            if re.fullmatch(r'content://media/(external|external_primary)/downloads/\d+',uri):shell('content delete --uri '+uri,check=False)
            else:assert uri=='content://com.vinaywinai.databeam.test.sourcefixture/tree/root',uri
        for session in record.get('sessions',[]):
            uuid.UUID(session)
            adb('shell','run-as',APP,'rm','-f',f'files/transfer-records/{session}.json',f'files/transfer-records/{session}.tmp',check=False)
            for parent in ('outgoing','received','blobs'):
                path=f'files/{parent}/{session}'
                real=shell(f'run-as {APP} readlink -f {path}',check=False).decode().strip()
                base=shell(f'run-as {APP} readlink -f files/{parent}',check=False).decode().strip()
                if real and base:
                    assert real==base+'/'+session
                    shell(f'run-as {APP} rm -rf {path}',check=False)
        path=f'files/restart-e2e/{case}'
        real=shell(f'run-as {APP} readlink -f {path}',check=False).decode().strip()
        base=shell(f'run-as {APP} readlink -f files/restart-e2e',check=False).decode().strip()
        if real and base:
            assert real==base+'/'+case;shell(f'run-as {APP} rm -rf {path}',check=False)
        restore()
    try:
        print('Backup:',work,flush=True)
        print(adb('install','-r','-t',ROOT/'android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk').decode().strip(),flush=True)
        relay_binary=work/'croc-relay.exe';shutil.copyfile(CROC,relay_binary)
        defaults=[(m,s) for s in ('stop','closed') for m in ('Croc','Sendme','EasySendMe')]+[('Croc','text')]+[(m,'mid') for m in ('Croc','Sendme','EasySendMe')]
        cases=[tuple(c.split(':',1)) for c in args.case] if args.case else defaults
        for mode,scenario in cases:
            assert mode in ('Croc','Sendme','EasySendMe') and scenario in ('stop','closed','text','mid')
            case=str(uuid.uuid4());case_ids.append(case);started=time.monotonic();case_procs=len(procs)
            kind='text' if scenario=='text' else args.kind or ('folder' if scenario=='closed' else 'flat')
            print('START',mode,scenario,flush=True)
            record=dict(mode=mode,scenario=scenario,kind=kind,run=case,status='FAIL')
            try:
                phone=instrument(case,mode,scenario,kind,mib=args.mib or ((128 if mode=='Croc' else 4) if scenario=='mid' else 1))
                first=wait_record(case,'prepared');initial=first['prepared']
                destination=work/(case+'-received');cache=work/(case+'-receiver-cache')
                partial_bytes=None
                if scenario=='mid':
                    pc=receiver(case,mode,initial,'initial',destination,cache)
                    end=time.monotonic()+120
                    while time.monotonic()<end:
                        samples=progress(case,'initial',mode)
                        if samples and max(samples)>=(8*1024*1024 if mode=='Croc' else 128*1024):
                            assert pc.poll() is None,'Initial transfer finished before interruption';partial_bytes=max(samples);break
                        if pc.poll() is not None:raise AssertionError('Initial receiver ended before a partial transfer')
                        time.sleep(.05)
                    else:raise AssertionError('No partial payload progress')
                    # A native sender remains open after a completed download.
                    # Stop the receiver immediately so it cannot finish filling
                    # its cache while the phone processes the restart command.
                    if mode!='Croc':terminate(pc)
                    command(case,'restart',1)
                    next_record=wait_record(case,'restarted')
                    try:pc.wait(timeout=3)
                    except subprocess.TimeoutExpired:terminate(pc)
                elif scenario=='closed':
                    app_pid=adb('shell','pidof',APP).decode().strip().split()[0]
                    processes=adb('shell','ps','-A','-o','PID,PPID,NAME').decode()
                    children=[int(p.split()[0]) for p in processes.splitlines()[1:] if len(p.split())>=3 and p.split()[1]==app_pid and 'croc' in p.split()[2]]
                    adb('shell','am','force-stop',APP)
                    phone.wait(timeout=15)
                    # A process-death restart must not leave its old Croc sender serving.
                    time.sleep(1)
                    live=adb('shell','ps','-A','-o','PID,NAME').decode().splitlines()
                    assert not any(line.split() and int(line.split()[0]) in children for line in live[1:]),'Croc child survived app closure'
                    phone=instrument(case,mode,scenario,kind,stage='continue')
                    next_record=wait_record(case,'restarted')
                else:
                    command(case,'restart',1);next_record=wait_record(case,'restarted')
                restarted=next_record['restarted'];restarted['scenario']=scenario
                assert restarted['id']!=initial['id'],'Same sender session reused'
                if mode!='Croc':assert restarted['ticket']!=initial['ticket'],'Old ticket was reused'
                else:assert restarted['code']==initial['code'],'Croc code unexpectedly changed'
                pc=receiver(case,mode,restarted,'restarted',destination,cache)
                pc.wait(timeout=190);assert pc.returncode==0,'Restarted receiver failed; inspect its log'
                total=verify(case,mode,next_record,destination,'restarted')
                samples=progress(case,'restarted',mode)
                if scenario=='mid':
                    assert samples and samples[0]>=64*1024,f'Receiver restarted at zero rather than cached bytes: {samples[:3]}'
                command(case,'finish',2,verified=True);wait_record(case,'passed')
                phone.wait(timeout=20)
                output=(work/(case+'-phone-'+('continue' if scenario=='closed' else 'start')+'.out')).read_text(encoding='utf8',errors='replace')
                assert 'INSTRUMENTATION_RESULT: result=PASS' in output,output[-1200:]
                record.update(status='PASS',bytes=total,files=len(next_record['files']),empty_directories=len(next_record['directories']),initial_partial_bytes=partial_bytes,resumed_from=samples[0] if scenario=='mid' else None,new_ticket=mode!='Croc',elapsed_seconds=round(time.monotonic()-started,2))
                print(json.dumps(record),flush=True)
            except Exception as error:
                (work/(case+'-failure-state.json')).write_text(json.dumps(state(),indent=2,ensure_ascii=False),encoding='utf8')
                record.update(error=str(error),elapsed_seconds=round(time.monotonic()-started,2));print(json.dumps(record),flush=True)
            finally:
                results.append(record);(work/'results.json').write_text(json.dumps(results,indent=2),encoding='utf8')
                for p in reversed(procs[case_procs:]):terminate(p)
                cleanup_case(case)
        assert all(r['status']=='PASS' for r in results),'Some restart cases failed; see results.json'
        print('PASS',len(results),'end-to-end restart cases',flush=True)
    finally:
        for p in reversed(procs):terminate(p)
        for f in handles:f.close()
        restore()
        print('Host preferences restored; test processes stopped.',flush=True)

if __name__=='__main__':main()
