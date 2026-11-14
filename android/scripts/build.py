"""Build the arm64 Android engines. Uses only the pinned Croc source/patch recipe."""
import argparse, hashlib, json, os, shutil, subprocess, urllib.request, zipfile
from pathlib import Path
ANDROID=Path(__file__).resolve().parents[1]
ROOT=ANDROID.parent

def run(args,**kw):
    print('>',*map(str,args),flush=True)
    subprocess.run(list(map(str,args)),check=True,**kw)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--sdk',type=Path,required=True)
    parser.add_argument('--ndk-version',default='28.2.13676358')
    args=parser.parse_args()
    ndk=args.sdk/'ndk'/args.ndk_version
    prebuilt=ndk/'toolchains/llvm/prebuilt'/('windows-x86_64' if os.name=='nt' else 'linux-x86_64')
    clang=prebuilt/'bin'/('aarch64-linux-android26-clang.cmd' if os.name=='nt' else 'aarch64-linux-android26-clang')
    if not clang.exists():raise SystemExit(f'Install NDK (side by side) {args.ndk_version} in Android Studio SDK Manager first: {clang}')
    output=ANDROID/'app/src/main/jniLibs/arm64-v8a';output.mkdir(parents=True,exist_ok=True)
    env=dict(os.environ)
    env['CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER']=str(clang)
    env['CC_aarch64_linux_android']=str(clang)
    env['AR_aarch64_linux_android']=str(prebuilt/'bin'/('llvm-ar.exe' if os.name=='nt' else 'llvm-ar'))
    run(['cargo','build','--manifest-path',ANDROID/'native/Cargo.toml','--target','aarch64-linux-android','--release'],env=env,cwd=ROOT)
    shutil.copy2(ANDROID/'native/target/aarch64-linux-android/release/libdatabeam_android.so',output)
    recipe=json.loads((ANDROID/'croc-engine.json').read_text())
    work=ANDROID/'.tools/croc';work.mkdir(parents=True,exist_ok=True)
    archive=work/(recipe['upstream_tag']+'.zip')
    if not archive.exists() and (ROOT/'target/croc-build'/ (recipe['upstream_tag']+'.zip')).exists():
        shutil.copy2(ROOT/'target/croc-build'/(recipe['upstream_tag']+'.zip'),archive)
    if not archive.exists():urllib.request.urlretrieve('https://codeload.github.com/schollz/croc/zip/refs/tags/'+recipe['upstream_tag'],archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest()!=recipe['source_sha256']:raise SystemExit('Croc source checksum mismatch')
    source=work/('croc-'+recipe['upstream_tag'].removeprefix('v'))
    if not source.exists():
        with zipfile.ZipFile(archive) as z:
            for item in z.infolist():
                if not (work/item.filename).resolve().is_relative_to(work.resolve()):raise SystemExit('Invalid archive path')
            z.extractall(work)
    stamp=source/'.databeam-patch'
    patch=ROOT/'patches'/recipe['patch']
    digest=hashlib.sha256(patch.read_bytes()).hexdigest()
    if not stamp.exists():
        run(['git','-c','safe.directory='+ROOT.as_posix(),'apply','--directory='+source.relative_to(ROOT).as_posix(),patch],cwd=ROOT)
        stamp.write_text(digest)
    elif stamp.read_text()!=digest:raise SystemExit('Croc patch changed; remove android/.tools/croc and rebuild')
    env.update(GOOS='android',GOARCH='arm64',CGO_ENABLED='1',CC=str(clang),GOTOOLCHAIN='go'+recipe['go_version'])
    # Android satisfies Go's linux tag, but cannot use Tailcat's Linux netlink
    # monitor. Its optional netcheck panics with a nil InterfaceState. Upstream's
    # supported build tag keeps classic LAN Direct and Croc relay transport.
    run(['go','build','-tags','croc_no_tailcat','-buildmode=pie','-trimpath','-buildvcs=false','-ldflags','-s -w -X github.com/schollz/croc/v11/src/cli.Version='+recipe['engine_version'],'-o',output/'libcroc.so','.'],env=env,cwd=source)
    print('Native engines built. Open android/ in Android Studio, or run gradlew.bat assembleDebug.')
if __name__=='__main__':main()
