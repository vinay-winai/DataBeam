"""Physical-device cache choices: pause/continue, offline complete export, delete/redownload.

All input/output provenance is saved before cleanup. The production app makes no
automatic choice; instrumentation clicks the visible dialog on behalf of this test.
"""
from pathlib import Path
import argparse,base64,hashlib,json,os,re,subprocess,time,uuid,xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[2]
ADB='X:/asproj/platform-tools/adb.exe'
APP='com.vinaywinai.databeam'
PEER=ROOT/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe'
CROC=ROOT/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe'

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--croc-only',action='store_true');args=parser.parse_args()
    work=ROOT/'android/.tools/smoke/receive-cache-e2e'/str(uuid.uuid4());work.mkdir(parents=True)
    print('Artifacts:',work,flush=True)
    def adb(*args,data=None,check=True):
        p=subprocess.run([ADB,*map(str,args)],input=data,capture_output=True,timeout=30)
        if check and p.returncode:raise RuntimeError(p.stderr.decode(errors='replace'))
        return p.stdout
    def state():
        xml=adb('exec-out','run-as',APP,'cat','shared_prefs/transfer.xml',check=False)
        if not xml.lstrip().startswith(b'<?xml'):return {}
        node=ET.fromstring(xml).find("string[@name='state']");return json.loads(node.text) if node is not None else {}
    assert state().get('status') not in ('running','stopping'),'A user transfer is active'
    snapshots={name:adb('exec-out','run-as',APP,'cat','shared_prefs/'+name,check=False) for name in ('settings.xml','transfer.xml')}
    snapshots={name:data if data.lstrip().startswith(b'<?xml') else b'' for name,data in snapshots.items()}
    for name,data in snapshots.items():(work/name).write_bytes(data)
    procs=[];handles=[];sessions=[];records=[];results=[]
    def spawn(command,label,env=None):
        out=(work/(label+'.out')).open('wb');err=(work/(label+'.err')).open('wb');handles.extend([out,err])
        p=subprocess.Popen(list(map(str,command)),stdout=out,stderr=err,env=env,creationflags=0x08000000);procs.append(p);return p
    def stop(p):
        if p.poll() is None:p.terminate();p.wait(timeout=10)
    def sender(fixture,label):
        p=spawn([PEER,'send',fixture,work/(label+'-sender-cache')],label+'-sender')
        end=time.monotonic()+30
        while time.monotonic()<end:
            text=(work/(label+'-sender.out')).read_text(encoding='utf8',errors='replace')
            match=re.search(r'^TICKET=(blob[a-z2-7]+)',text,re.M)
            if match:return p,match.group(1)
            assert p.poll() is None,'Sender failed';time.sleep(.1)
        raise AssertionError('No sender ticket')
    def receive(mode,ticket,label,fixture,expected,session=None,**options):
        case=session or str(uuid.uuid4())
        if case not in sessions:sessions.append(case)
        config=dict(id=case,mode=mode,action='receive',ticket=ticket,testCacheChoice='continue',testTimeoutSeconds=170)
        config.update(options)
        if mode=='EasySendMe':
            code='cache-'+uuid.uuid4().hex[:12];config.update(code=code,relay='1.getcroc.com:9009');config.pop('ticket')
            spawn([CROC,'--yes','--ignore-stdin','--relay',config['relay'],'send','--text',ticket],label+'-handoff',dict(os.environ,CROC_SECRET=code,CROC_NO_UPDATE_CHECK='1'))
        payload=base64.b64encode(json.dumps(config).encode()).decode()
        phone=spawn([ADB,'shell','am','instrument','-w','-e','config',payload,APP+'.test/'+APP+'.SmokeInstrumentation'],label+'-phone')
        try:phone.wait(timeout=190)
        finally:
            s=state()
            if s.get('id')==case:records.append(s);(work/(label+'-state.json')).write_text(json.dumps(s,indent=2,ensure_ascii=False),encoding='utf8')
        output=(work/(label+'-phone.out')).read_text(encoding='utf8',errors='replace')
        assert 'INSTRUMENTATION_CODE: 0' in output and 'shortMsg=' not in output and 'download_hash_' in output,output[-2000:]
        assert f'download_hash_{fixture.name}={expected}' in output,'Published SHA-256 mismatch'
        if options.get('pauseTest') or options.get('requireCachedResume'):
            found=re.search(r'INSTRUMENTATION_RESULT: resumed_from=(\d+)',output)
            assert found and int(found.group(1))>0,'Progress did not resume above zero'
        if options.get('testCacheChoice')=='delete':
            samples=re.findall(r'receive-progress (\d+):',s.get('log',''));assert samples and int(samples[0])==0,'Delete reused data instead of starting from zero'
        results.append(dict(test=label,status='PASS',id=case,sha256=expected,prompt_bytes=re.findall(r'cache_prompt_bytes=(\d+)',output),resumed_from=re.findall(r'resumed_from=(\d+)',output)))
        (work/'results.json').write_text(json.dumps(results,indent=2),encoding='utf8');print('PASS',label,flush=True)
        return s
    try:
        if args.croc_only:
            fixture=work/('croc-reconnect-'+uuid.uuid4().hex[:8]+'.bin')
            block=bytes(i%251 for i in range(1024*1024));digest=hashlib.sha256()
            with fixture.open('wb') as f:
                for _ in range(64):f.write(block);digest.update(block)
            expected=digest.hexdigest();case=str(uuid.uuid4());sessions.append(case)
            code='reconnect-'+uuid.uuid4().hex[:12];relay='1.getcroc.com:9009'
            env=dict(os.environ,CROC_SECRET=code,CROC_NO_UPDATE_CHECK='1')
            send_command=[CROC,'--yes','--ignore-stdin','--relay',relay,'--throttleUpload','10m','send','--',fixture]
            peer=spawn(send_command,'croc-initial',env)
            config=dict(id=case,mode='Croc',action='receive',code=code,relay=relay)
            payload=base64.b64encode(json.dumps(config).encode()).decode()
            phone=spawn([ADB,'shell','am','instrument','-w','-e','config',payload,APP+'.test/'+APP+'.SmokeInstrumentation'],'croc-initial-phone')
            end=time.monotonic()+65
            while time.monotonic()<end:
                saved=state()
                if saved.get('id')==case and saved.get('done',0)>=8*1024*1024:
                    assert saved.get('status')=='running','Transfer finished before interruption';break
                if saved.get('id')==case and saved.get('status')=='interrupted':raise AssertionError(saved.get('message'))
                time.sleep(.1)
            else:raise AssertionError('No Croc payload before interruption')
            adb('shell','am','force-stop',APP);phone.wait(timeout=15);stop(peer)
            saved=state();records.append(saved)
            print('Croc interrupted at',saved.get('done'),flush=True)
            spawn(send_command,'croc-restarted',env)
            receive('Croc','','croc-app-close-resume',fixture,expected,session=case,code=code,relay=relay,requireCachedResume=True)
        for mode in (() if args.croc_only else ('Sendme','EasySendMe')):
            label=mode.lower();fixture=work/(label+'-'+uuid.uuid4().hex[:8]+'.bin')
            block=bytes(i%251 for i in range(1024*1024));digest=hashlib.sha256()
            with fixture.open('wb') as f:
                for _ in range(256):f.write(block);digest.update(block)
            expected=digest.hexdigest();peer,ticket=sender(fixture,label+'-initial')
            receive(mode,ticket,label+'-partial',fixture,expected,pauseTest=True,requireCachePrompt=True,expectCompleteCache=False)
            stop(peer)
            # A new session discovers the old complete cache after the sender has gone.
            receive('Sendme',ticket,label+'-offline-complete',fixture,expected,requireCachePrompt=True,expectCompleteCache=True,requireCachedResume=True)
            # Test deletion with a small independent transfer to avoid a second large download.
            small=work/(label+'-delete-'+uuid.uuid4().hex[:8]+'.bin');small.write_bytes(block)
            small_hash=hashlib.sha256(block).hexdigest();peer,small_ticket=sender(small,label+'-delete')
            receive(mode,small_ticket,label+'-delete-seed',small,small_hash)
            # The large, unrelated complete cache must survive this delete choice.
            receive(mode,small_ticket,label+'-delete-choice',small,small_hash,testCacheChoice='delete',requireCachePrompt=True,expectCompleteCache=True)
            stop(peer)
            receive('Sendme',ticket,label+'-unrelated-cache-kept',fixture,expected,requireCachePrompt=True,expectCompleteCache=True,requireCachedResume=True)
        print('PASS all receive-cache choices',flush=True)
    finally:
        for p in reversed(procs):stop(p)
        adb('shell','am','force-stop',APP,check=False)
        for s in records:
            for uri in s.get('published',{}).values():
                if re.fullmatch(r'content://media/(external|external_primary)/downloads/\d+',uri):adb('shell','content','delete','--uri',uri,check=False)
        for case in sessions:
            uuid.UUID(case)
            adb('shell','run-as',APP,'rm','-f',f'files/transfer-records/{case}.json',f'files/transfer-records/{case}.tmp',check=False)
            for parent in ('blobs','received'):
                path=f'files/{parent}/{case}'
                real=adb('shell','run-as',APP,'readlink','-f',path,check=False).decode().strip()
                base=adb('shell','run-as',APP,'readlink','-f','files/'+parent,check=False).decode().strip()
                if real and base:
                    assert real==base+'/'+case;adb('shell','run-as',APP,'rm','-rf',path,check=False)
        for name,data in snapshots.items():
            if data:
                adb('shell','run-as',APP,'mkdir','-p','shared_prefs')
                adb('shell','run-as',APP,'tee','shared_prefs/'+name,data=data)
            else:adb('shell','run-as',APP,'rm','-f','shared_prefs/'+name,check=False)
        adb('shell','run-as',APP,'rm','-f','shared_prefs/transfer.xml.bak','shared_prefs/settings.xml.bak',check=False)
        for f in handles:f.close()
        print('Preferences restored; generated phone outputs removed.',flush=True)

if __name__=='__main__':main()
