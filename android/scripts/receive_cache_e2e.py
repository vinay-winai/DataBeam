"""Physical-device automatic cached receives and Croc reconnect; generated data only."""
from pathlib import Path
import argparse,base64,hashlib,json,os,re,subprocess,time,uuid,xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[2]
ADB='X:/asproj/platform-tools/adb.exe'
APP='com.vinaywinai.databeam'
PEER=ROOT/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe'
CROC=ROOT/'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe'

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--croc-only',action='store_true');parser.add_argument('--mode',choices=('Sendme','EasySendMe'));args=parser.parse_args()
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
    snapshots={}
    for name in ('settings.xml','transfer.xml'):
        read=subprocess.run([ADB,'exec-out','run-as',APP,'cat','shared_prefs/'+name],capture_output=True,timeout=30)
        if read.returncode:
            error=(read.stdout+read.stderr).decode(errors='replace')
            if 'No such file or directory' not in error:raise RuntimeError(error)
            snapshots[name]=b''
        else:ET.fromstring(read.stdout);snapshots[name]=read.stdout
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
        config=dict(id=case,mode=mode,action='receive',ticket=ticket,testTimeoutSeconds=170)
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
        results.append(dict(test=label,status='PASS',id=case,sha256=expected,cached_bytes=re.findall(r'cached_receive_bytes=(\d+)',output),resumed_from=re.findall(r'resumed_from=(\d+)',output)))
        (work/'results.json').write_text(json.dumps(results,indent=2),encoding='utf8');print('PASS',label,flush=True)
        return s
    try:
        if args.croc_only:
            fixture=work/('croc-reconnect-'+uuid.uuid4().hex[:8]+'.bin')
            digest=hashlib.sha256()
            with fixture.open('wb') as f:
                for _ in range(48):
                    block=os.urandom(1024*1024);f.write(block);digest.update(block)
            expected=digest.hexdigest();case=str(uuid.uuid4());sessions.append(case)
            code='reconnect-'+uuid.uuid4().hex[:12];relay='1.getcroc.com:9009'
            env=dict(os.environ,CROC_SECRET=code,CROC_NO_UPDATE_CHECK='1',DATABEAM_CROC_FILE_DIGEST='1')
            send_command=[CROC,'--yes','--ignore-stdin','--relay',relay,'--throttleUpload','6m','send','--',fixture]
            peer=spawn(send_command,'croc-initial',env)
            config=dict(id=case,mode='Croc',action='receive',code=code,relay=relay,saveFolder='',saveFolderLabel='Downloads / DataBeam')
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
            assert saved.get('crocDirect'),saved.get('message','Direct Downloads not enabled')
            assert saved.get('crocPending'), 'No pending Downloads rows'
            original_rows=saved['crocPending']
            assert not adb('shell','run-as',APP,'ls','-A',f'files/received/{case}').strip(),'Payload duplicated in private storage'
            print('Croc interrupted at',saved.get('done'),flush=True)
            wrong_fixture=work/'different-content'/fixture.name
            wrong_fixture.parent.mkdir()
            with fixture.open('rb') as original, wrong_fixture.open('wb') as changed:
                changed.write(bytes([original.read(1)[0]^1]))
                while block:=original.read(1024*1024):changed.write(block)
            prior=dict(config,receiveCode=code,crocDirect=True,crocPending=original_rows,crocWrapper=saved['crocWrapper'])
            for label, command in (
                ('different-file',send_command[:-1]+[wrong_fixture]),
                ('text',send_command[:-2]+['--text','Generated Croc resume test']),
            ):
                different='different-'+uuid.uuid4().hex[:12]
                wrong_sender=spawn(command,'croc-'+label,dict(env,CROC_SECRET=different))
                payload=base64.b64encode(json.dumps(dict(prior,code=different)).encode()).decode()
                wrong_phone=spawn([ADB,'shell','am','instrument','-w','-e','config',payload,APP+'.test/'+APP+'.SmokeInstrumentation'],'croc-'+label+'-phone')
                wrong_phone.wait(timeout=70)
                rejected=state()
                assert rejected.get('id')==case and rejected.get('status')=='interrupted', 'Mismatched payload was accepted'
                assert 'sharing different files' in rejected.get('message',''),rejected.get('message')
                assert rejected.get('crocPending')==original_rows,'Mismatched payload changed the stored files'
                stop(wrong_sender)
                print('PASS rejected mismatched sender offering '+label,flush=True)
            replacement='reconnect-'+uuid.uuid4().hex[:12]
            spawn(send_command,'croc-restarted',dict(env,CROC_SECRET=replacement))
            completed=receive('Croc','','croc-app-close-new-code-resume',fixture,expected,session=case,
                code=replacement,receiveCode=code,relay=relay,requireCachedResume=True,
                saveFolder='',saveFolderLabel='Downloads / DataBeam',crocDirect=True,
                crocPending=original_rows,crocWrapper=saved['crocWrapper'])
            assert completed.get('published')==original_rows,'Resume changed the download row'
            assert completed.get('firstPayloadDone',0)>=4*1024*1024,'Resume did not preserve the partial byte range'
            assert not adb('shell','run-as',APP,'ls',f'files/croc-resume/{case}.json',check=False).strip(),'Resume metadata kept after success'
        for mode in (() if args.croc_only else (args.mode,) if args.mode else ('Sendme','EasySendMe')):
            label=mode.lower();fixture=work/(label+'-'+uuid.uuid4().hex[:8]+'.bin')
            block=bytes(i%251 for i in range(1024*1024));digest=hashlib.sha256()
            with fixture.open('wb') as f:
                for _ in range(256):f.write(block);digest.update(block)
            expected=digest.hexdigest();peer,ticket=sender(fixture,label+'-initial')
            receive(mode,ticket,label+'-partial',fixture,expected,pauseTest=True,requireAutomaticCache=True)
            stop(peer)
        print('PASS automatic saved-data continuation',flush=True)
    finally:
        for p in reversed(procs):stop(p)
        adb('shell','am','force-stop',APP,check=False)
        for s in records:
            for uri in {*s.get('published',{}).values(),*s.get('crocPending',{}).values()}:
                if re.fullmatch(r'content://media/(external|external_primary)/downloads/\d+',uri):adb('shell','content','delete','--uri',uri,check=False)
            wrapper=s.get('crocWrapper','')
            if re.fullmatch(r'croc-reconnect-[a-z0-9]{5}_databeam(?: \(\d+\))?',wrapper):
                adb('shell','rmdir','/sdcard/Download/DataBeam/'+wrapper,check=False)
        for case in sessions:
            uuid.UUID(case)
            adb('shell','run-as',APP,'rm','-f',f'files/transfer-records/{case}.json',f'files/transfer-records/{case}.tmp',check=False)
            adb('shell','run-as',APP,'rm','-f',f'files/croc-resume/{case}.json',check=False)
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
