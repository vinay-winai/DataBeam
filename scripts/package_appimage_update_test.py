"""Build the fixed updater as v0.7.1, for upgrading to the existing stable release."""
import argparse
from pathlib import Path
import re
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--arch', choices=['x86_64', 'aarch64'], required=True)
args = parser.parse_args()
files = [root / 'Cargo.toml', root / 'Cargo.lock']
originals = {path: path.read_bytes() for path in files}
try:
    for path in files:
        source = originals[path].decode('utf-8')
        pattern = (r'(?m)^(version = ")[^"]+("\r?$)' if path.suffix == '.toml'
                   else r'(?m)(^name = "databeam"\r?\nversion = ")[^"]+(")')
        source, count = re.subn(pattern, r'\g<1>0.7.1\2', source, count=1)
        assert count == 1, f'Missing DataBeam version in {path}'
        path.write_bytes(source.encode('utf-8'))
    subprocess.run(['cargo', 'build', '--locked', '--release', '--features',
                    'release-single-instance', '--bin', 'databeam'], cwd=root, check=True)
    subprocess.run(['cargo', 'bundle', '--release', '--features', 'release-single-instance',
                    '--format', 'appimage', '--binary-path', 'target/release/databeam'],
                   cwd=root, check=True)
    packages = list((root / 'target/release/bundle/appimage').glob('*0.7.1*.AppImage'))
    assert len(packages) == 1, f'Expected one v0.7.1 AppImage, found {packages}'
    subprocess.run(['python3', str(root / 'scripts/bundle_appimage_runtime.py'),
                    str(packages[0])], cwd=root, check=True)
    label = {'x86_64': 'x64', 'aarch64': 'arm64'}[args.arch]
    destination = root / 'dist' / f'databeam_update-test-v0.7.1_linux-{label}.AppImage'
    destination.parent.mkdir(exist_ok=True)
    shutil.copy2(packages[0], destination)
    destination.chmod(0o755)
    print(f'Created fixed updater test: {destination}')
finally:
    for path, contents in originals.items():
        path.write_bytes(contents)
