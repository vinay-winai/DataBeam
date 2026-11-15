"""Phone UI checks for engine visibility, Resume choices and file-manager reuse."""
from pathlib import Path
import hashlib,json,re,subprocess,uuid,xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[2]
ADB='X:/asproj/platform-tools/adb.exe';APP='com.vinaywinai.databeam'

def main():
    work=ROOT/'android/.tools/smoke/settings-flow'/str(uuid.uuid4());work.mkdir(parents=True)
    print('Artifacts:',work,flush=True);saved={}
    def adb(*args,data=None,check=True,timeout=30):
        p=subprocess.run([ADB,*map(str,args)],input=data,capture_output=True,timeout=timeout)
        if check and p.returncode:raise RuntimeError((p.stdout+p.stderr).decode(errors='replace'))
        return p
    for name in ('settings.xml','transfer.xml','permissions.xml','troubleshoot-news.xml'):
        p=adb('exec-out','run-as',APP,'cat','shared_prefs/'+name,check=False)
        if p.returncode:
            if b'No such file or directory' not in p.stdout+p.stderr:raise RuntimeError((p.stdout+p.stderr).decode(errors='replace'))
            saved[name]=None
        else:ET.fromstring(p.stdout);saved[name]=p.stdout;(work/name).write_bytes(p.stdout)
    if saved['transfer.xml']:
        node=ET.fromstring(saved['transfer.xml']).find("string[@name='state']")
        assert node is None or json.loads(node.text).get('status') not in ('running','stopping'),'A user transfer is active'
    try:
        for package,path,test in ((APP,ROOT/'android/app/build/outputs/apk/debug/app-debug.apk',False),(APP+'.test',ROOT/'android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk',True)):
            location=adb('shell','pm','path',package,check=False).stdout.decode().strip().removeprefix('package:')
            installed=adb('shell','sha256sum',location,check=False).stdout.decode().split() if location.startswith('/data/app/') else []
            if not installed or installed[0]!=hashlib.sha256(path.read_bytes()).hexdigest():adb('install','-r',*(['-t'] if test else []),path)
        ticket=(ROOT/'android/app/src/test/resources/desktop-qr/sendme.txt').read_text().strip()
        folder_uri='content://com.android.externalstorage.documents/document/primary%3ADownload%2FDataBeam'
        handler=adb('shell','cmd','package','resolve-activity','--brief','-a','android.intent.action.VIEW','-d',folder_uri,'-t','vnd.android.document/directory').stdout.decode().strip().splitlines()[-1]
        package,activity=handler.split('/',1);activity=package+activity if activity.startswith('.') else activity
        base=adb('shell','pm','path',package).stdout.decode().splitlines()[0].removeprefix('package:')
        assert base.startswith('/') and base.endswith('.apk');apk=work/'folder-handler.apk';adb('pull',base,apk)
        manifest=subprocess.check_output(['X:/asproj/build-tools/36.0.0/aapt2.exe','dump','xmltree','--file','AndroidManifest.xml',apk],text=True)
        (work/'folder-handler-manifest.txt').write_text(manifest)
        launch_mode=0
        for block in re.split(r'(?m)^\s*E: activity(?:-alias)? \(',manifest):
            if re.search(r':name\([^\n]*="'+re.escape(activity)+r'"',block):
                found=re.search(r':documentLaunchMode\([^\n]*=([^\s]+)',block)
                launch_mode=int(found.group(1),0) if found else 0;break
        (work/'folder-policy.json').write_text(json.dumps(dict(component=handler,documentLaunchMode=launch_mode)))
        p=adb('shell','am','instrument','-w','-e','settings_flow_test','true','-e','folder_handler',handler,'-e','folder_document_mode',launch_mode,'-e','ticket',ticket,APP+'.test/'+APP+'.SmokeInstrumentation',timeout=90)
        output=p.stdout.decode(errors='replace');(work/'phone.out').write_text(output)
        print(output[-4500:],flush=True)
        for name in ('settings-engines-test.png','easy-resume-prompt-test.png'):
            png=adb('exec-out','run-as',APP,'cat','cache/'+name,check=False).stdout
            if png.startswith(b'\x89PNG'):(work/name).write_bytes(png)
        assert 'INSTRUMENTATION_RESULT: result=PASS' in output and 'INSTRUMENTATION_CODE: 0' in output,output[-4500:]
    finally:
        adb('shell','am','force-stop',APP,check=False)
        for name,data in saved.items():
            if data is None:adb('shell','run-as',APP,'rm','-f','shared_prefs/'+name)
            else:adb('shell','run-as',APP,'tee','shared_prefs/'+name,data=data)
            adb('shell','run-as',APP,'rm','-f','shared_prefs/'+name+'.bak')
        for name in ('settings-engines-test.png','easy-resume-prompt-test.png'):adb('shell','run-as',APP,'rm','-f','cache/'+name,check=False)
        print('Original settings, transfer, permissions and guide preferences restored.',flush=True)

if __name__=='__main__':main()
