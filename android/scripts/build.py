"""Build the arm64 Android engines from pinned dependencies and patches."""
import argparse, hashlib, json, os, shutil, subprocess, urllib.request, zipfile
import tarfile, tempfile
from pathlib import Path, PurePosixPath
ANDROID=Path(__file__).resolve().parents[1]
ROOT=ANDROID.parent

def run(args,**kw):
    print('>',*map(str,args),flush=True)
    subprocess.run(list(map(str,args)),check=True,**kw)


def prepare_iroh_blobs():
    # Match android/native/Cargo.lock. The path override preserves the locked
    # dependency graph and applies only to this Android engine build.
    version = '0.103.1'
    crate = f'iroh-blobs-{version}'
    sha256 = 'a3148ad01585eb6b7cfb641e4100564f4f56db644f4cbed8c10ab2478663161f'
    patch = ROOT / 'patches' / f'iroh-blobs-v{version}-android-sources.patch'
    patch_hash = hashlib.sha256(patch.read_bytes()).hexdigest()
    work = ANDROID / '.tools/iroh-blobs'
    source = work / patch_hash / crate
    if source.is_dir():
        return source

    source.parent.mkdir(parents=True, exist_ok=True)
    cargo_home = Path(os.environ.get('CARGO_HOME', Path.home() / '.cargo'))
    cached = next((cargo_home / 'registry/cache').glob(f'*/{crate}.crate'), None)
    with tempfile.TemporaryDirectory(prefix='source-', dir=work) as temp:
        destination = Path(temp)
        archive = cached or destination / f'{crate}.crate'
        if cached is None:
            url = f'https://static.crates.io/crates/iroh-blobs/{crate}.crate'
            urllib.request.urlretrieve(url, archive)
        if hashlib.sha256(archive.read_bytes()).hexdigest() != sha256:
            raise SystemExit('Iroh blobs source checksum mismatch')
        with tarfile.open(archive, 'r:gz') as package:
            for item in package.getmembers():
                parts = PurePosixPath(item.name).parts
                if (not parts or parts[0] != crate or '..' in parts or '\\' in item.name or ':' in item.name
                        or not (item.isdir() or item.isfile())):
                    raise SystemExit('Unsafe Iroh blobs archive entry')
            package.extractall(destination)
        extracted = destination / crate
        run(['git', '-c', 'safe.directory=' + ROOT.as_posix(), 'apply',
             '--directory=' + extracted.relative_to(ROOT).as_posix(), patch], cwd=ROOT)
        try:
            extracted.rename(source)
        except OSError:
            if not source.is_dir():
                raise
    return source

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
    iroh_blobs = prepare_iroh_blobs()
    override = 'paths=' + json.dumps([iroh_blobs.as_posix()])
    run(['cargo','build','--manifest-path',ANDROID/'native/Cargo.toml','--locked',
         '--config',override,'--features','descriptor-sources','--target','aarch64-linux-android','--release'],env=env,cwd=ROOT)
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
    source_patch=ROOT/'patches'/recipe['source_patch']
    source_stamp=source/'.databeam-source-patch'
    source_digest=hashlib.sha256(source_patch.read_bytes()).hexdigest()
    if not source_stamp.exists():
        run(['git','-c','safe.directory='+ROOT.as_posix(),'apply','--directory='+source.relative_to(ROOT).as_posix(),source_patch],cwd=ROOT)
        source_stamp.write_text(source_digest)
    elif source_stamp.read_text()!=source_digest:raise SystemExit('Android source adapter changed; rebuild from fresh pinned Croc source')
    route_patch=ROOT/'patches'/recipe['route_patch']
    route_stamp=source/'.databeam-route-patch'
    route_digest=hashlib.sha256(route_patch.read_bytes()).hexdigest()
    if not route_stamp.exists():
        run(['git','-c','safe.directory='+ROOT.as_posix(),'apply','--directory='+source.relative_to(ROOT).as_posix(),route_patch],cwd=ROOT)
        route_stamp.write_text(route_digest)
    elif route_stamp.read_text()!=route_digest:raise SystemExit('Android local route patch changed; rebuild from fresh pinned Croc source')
    receive_patch=ROOT/'patches'/recipe['receive_patch']
    receive_stamp=source/'.databeam-receive-patch'
    receive_digest=hashlib.sha256(receive_patch.read_bytes()).hexdigest()
    if not receive_stamp.exists():
        run(['git','-c','safe.directory='+ROOT.as_posix(),'apply','--directory='+source.relative_to(ROOT).as_posix(),receive_patch],cwd=ROOT)
        receive_stamp.write_text(receive_digest)
    elif receive_stamp.read_text()!=receive_digest:raise SystemExit('Croc receive patch changed; rebuild from fresh pinned Croc source')
    downloads_patch=ROOT/'patches'/recipe['downloads_patch']
    downloads_stamp=source/'.databeam-downloads-patch'
    downloads_digest=hashlib.sha256(downloads_patch.read_bytes()).hexdigest()
    if not downloads_stamp.exists():
        run(['git','-c','safe.directory='+ROOT.as_posix(),'apply','--directory='+source.relative_to(ROOT).as_posix(),downloads_patch],cwd=ROOT)
        downloads_stamp.write_text(downloads_digest)
    elif downloads_stamp.read_text()!=downloads_digest:raise SystemExit('Croc Downloads patch changed; rebuild from fresh pinned Croc source')
    revision_patch=ROOT/'patches'/recipe['revision_patch']
    revision_stamp=source/'.databeam-revision-patch'
    revision_digest=hashlib.sha256(revision_patch.read_bytes()).hexdigest()
    if not revision_stamp.exists():
        run(['git','-c','safe.directory='+ROOT.as_posix(),'apply','--directory='+source.relative_to(ROOT).as_posix(),revision_patch],cwd=ROOT)
        revision_stamp.write_text(revision_digest)
    elif revision_stamp.read_text()!=revision_digest:raise SystemExit('Croc batch revision patch changed; rebuild from fresh pinned Croc source')
    fd_patch=ROOT/'patches'/recipe['source_fd_patch']
    fd_stamp=source/'.databeam-source-fd-patch'
    fd_digest=hashlib.sha256(fd_patch.read_bytes()).hexdigest()
    if not fd_stamp.exists():
        run(['git','-c','safe.directory='+ROOT.as_posix(),'apply','--directory='+source.relative_to(ROOT).as_posix(),fd_patch],cwd=ROOT)
        fd_stamp.write_text(fd_digest)
    elif fd_stamp.read_text()!=fd_digest:raise SystemExit('Croc descriptor adapter changed; rebuild from fresh pinned Croc source')
    env.update(GOOS='android',GOARCH='arm64',CGO_ENABLED='1',CC=str(clang),GOTOOLCHAIN='go'+recipe['go_version'])
    # Android satisfies Go's linux tag, but cannot use Tailcat's Linux netlink
    # monitor. Its optional netcheck panics with a nil InterfaceState. Upstream's
    # supported build tag keeps classic LAN Direct and Croc relay transport.
    run(['go','build','-tags','croc_no_tailcat','-buildmode=pie','-trimpath','-buildvcs=false','-ldflags','-s -w -X github.com/schollz/croc/v11/src/cli.Version='+recipe['engine_version'],'-o',output/'libcroc.so','.'],env=env,cwd=source)
    print('Native engines built. Open android/ in Android Studio, or run gradlew.bat assembleDebug.')
if __name__=='__main__':main()
