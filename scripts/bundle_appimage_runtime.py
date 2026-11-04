"""Include the dynamically loaded XWayland libraries in a cargo-bundle AppImage."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import tempfile

LIBRARIES = {
    'libxkbcommon-x11.so.0': 'libxkbcommon-x11-0',
    'libxkbcommon.so.0': 'libxkbcommon0',
    'libxcb-xkb.so.1': 'libxcb-xkb1',
}


def bundle(image: Path):
    if platform.system() != 'Linux':
        raise RuntimeError('AppImage runtime packaging requires native Linux')
    image = image.resolve(strict=True)
    appdir = image.with_suffix('') / 'AppDir'
    executable = appdir / 'usr/bin/databeam'
    if not executable.is_file():
        raise RuntimeError(f'Expected cargo-bundle staging directory: {appdir}')
    header = image.read_bytes()[:20]
    machine = struct.unpack('<H', header[18:20])[0]
    if header[:4] != b'\x7fELF' or machine not in (62, 183):
        raise RuntimeError('Expected x64 or ARM64 ELF AppImage')
    offset = int(subprocess.check_output([str(image), '--appimage-offset'], text=True).strip())
    with image.open('rb') as original:
        runtime = original.read(offset)
        if offset < 65536 or len(runtime) != offset or original.read(4) != b'hsqs':
            raise RuntimeError('Invalid AppImage runtime/SquashFS boundary')

    cache = subprocess.check_output(['ldconfig', '-p'], text=True)
    library_dir = appdir / 'usr/lib'
    library_dir.mkdir(parents=True, exist_ok=True)
    for soname, package in LIBRARIES.items():
        matches = [Path(line.split('=>', 1)[1].strip()) for line in cache.splitlines()
                   if '=>' in line and line.split()[0] == soname]
        source = next((path for path in matches
                       if struct.unpack('<H', path.read_bytes()[18:20])[0] == machine), None)
        if source is None:
            raise RuntimeError(f'Missing native {soname}; install {package} before packaging')
        shutil.copy2(source, library_dir / soname)
        copyright_file = Path('/usr/share/doc') / package / 'copyright'
        license_dir = appdir / 'usr/share/licenses/databeam'
        license_dir.mkdir(parents=True, exist_ok=True)
        shutil.copy2(copyright_file, license_dir / f'{package}-copyright')

    # Probe the same library search path the AppRun wrapper exports. The map
    # check prevents a build machine's installed library from hiding a gap.
    probe = """import ctypes, pathlib, sys
ctypes.CDLL('libxkbcommon-x11.so.0')
maps = pathlib.Path('/proc/self/maps').read_text()
for soname in sys.argv[2:]:
    assert str(pathlib.Path(sys.argv[1], soname).resolve()) in maps, soname
"""
    environment = os.environ.copy()
    environment['LD_LIBRARY_PATH'] = str(library_dir)
    subprocess.run(['python3', '-c', probe, str(library_dir), *LIBRARIES],
                   env=environment, check=True)
    apprun = appdir / 'AppRun'
    if apprun.is_symlink():
        apprun.unlink()
    apprun.write_text('''#!/bin/sh
set -eu
appdir="${APPDIR:-$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)}"
export LD_LIBRARY_PATH="$appdir/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$appdir/usr/bin/databeam" "$@"
''', encoding='utf-8')
    apprun.chmod(0o755)
    subprocess.run(['sh', '-n', str(apprun)], check=True)

    # Keep the verified runtime from cargo-bundle; rebuild only its filesystem.
    with tempfile.TemporaryDirectory(prefix='appimage-runtime-', dir=image.parent) as temporary:
        temporary = Path(temporary)
        filesystem = temporary / 'payload.squashfs'
        subprocess.run(['mksquashfs', str(appdir), str(filesystem), '-noappend',
                        '-comp', 'gzip', '-all-root', '-no-xattrs', '-no-progress',
                        '-processors', '2'], check=True)
        listing = subprocess.check_output(['unsquashfs', '-ll', str(filesystem)], text=True)
        for soname in LIBRARIES:
            if f'/usr/lib/{soname}' not in listing:
                raise RuntimeError(f'Packaged filesystem is missing {soname}')
        result = temporary / 'DataBeam.AppImage'
        with result.open('wb') as output, filesystem.open('rb') as payload:
            output.write(runtime)
            shutil.copyfileobj(payload, output)
        result.chmod(0o755)
        if int(subprocess.check_output([str(result), '--appimage-offset'], text=True)) != offset:
            raise RuntimeError('AppImage runtime changed during repackaging')
        os.replace(result, image)
        # Exercise the final wrapper and ELF without a display or FUSE. The
        # helper mode must start and report the intentionally missing plan.
        environment = os.environ.copy()
        environment['APPIMAGE_EXTRACT_AND_RUN'] = '1'
        environment.pop('DISPLAY', None)
        environment.pop('WAYLAND_DISPLAY', None)
        smoke = subprocess.run([str(image), '--databeam-apply-update', str(temporary / 'missing.json')],
                               env=environment, capture_output=True, text=True, timeout=60)
        if smoke.returncode != 0 or 'DataBeam update:' not in smoke.stderr:
            raise RuntimeError(f'Packaged updater helper failed to start: {smoke.stderr}')
    print(f'Bundled and checked XWayland runtime libraries: {image.name}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', type=Path)
    bundle(parser.parse_args().image)
