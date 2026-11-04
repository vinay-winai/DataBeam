"""Package a freshly built native Linux binary as an architecture-specific test Flatpak."""
import argparse
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--arch', choices=['x86_64', 'aarch64'], required=True)
parser.add_argument('--binary', type=Path, default=root / 'target/release/databeam')
args = parser.parse_args()
header = args.binary.read_bytes()[:20]
assert header[:4] == b'\x7fELF' and header[4:6] == b'\x02\x01', 'Expected a 64-bit Linux ELF'
assert struct.unpack('<H', header[18:20])[0] == {'x86_64': 62, 'aarch64': 183}[args.arch], 'Wrong architecture'
base = root / 'target/flatpak'
base.mkdir(parents=True, exist_ok=True)
stage = Path(tempfile.mkdtemp(prefix=args.arch + '-', dir=base))
payload = stage / 'payload'
payload.mkdir()
shutil.copy2(args.binary, payload / 'databeam')
for name in ['com.vinaywinai.databeam.json', 'com.vinaywinai.databeam.desktop', 'com.vinaywinai.databeam.metainfo.xml']:
    shutil.copy2(root / 'packaging/flatpak' / name, stage if name.endswith('.json') else payload)
for name in ['LICENSE', 'THIRD_PARTY_NOTICES.md']:
    shutil.copy2(root / name, payload)
shutil.copytree(root / 'LICENSES', payload / 'LICENSES')
shutil.copy2(root / 'assets/icons/icon-512.png', payload)
subprocess.run(['desktop-file-validate', str(payload / 'com.vinaywinai.databeam.desktop')], check=True)
subprocess.run(['appstream-util', 'validate-relax', '--nonet', str(payload / 'com.vinaywinai.databeam.metainfo.xml')], check=True)
subprocess.run(['flatpak-builder', '--user', '--force-clean', '--disable-rofiles-fuse',
    '--install-deps-from=flathub', '--default-branch=testing', '--repo=' + str(stage / 'repo'),
    str(stage / 'build'), str(stage / 'com.vinaywinai.databeam.json')], check=True)
dist = root / 'dist'
dist.mkdir(exist_ok=True)
name = 'databeam_linux-' + {'x86_64': 'x64', 'aarch64': 'arm64'}[args.arch] + '.flatpak'
subprocess.run(['flatpak', 'build-bundle', '--arch=' + args.arch,
    '--runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo', str(stage / 'repo'),
    str(dist / name), 'com.vinaywinai.databeam', 'testing'], check=True)
(base / ('build-' + args.arch + '.json')).write_text(json.dumps({'stage': str(stage), 'bundle': str(dist / name)}))
print('Created', dist / name)
