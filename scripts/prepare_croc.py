"""Build the pinned, patched Croc payload embedded by DataBeam's Rust build."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
RECIPE = json.loads((ROOT / 'patches/croc-engine.json').read_text())
TARGETS = {
    'x86_64-pc-windows-msvc': ('windows', 'amd64'),
    'aarch64-pc-windows-msvc': ('windows', 'arm64'),
    'x86_64-unknown-linux-gnu': ('linux', 'amd64'),
    'aarch64-unknown-linux-gnu': ('linux', 'arm64'),
    'x86_64-apple-darwin': ('darwin', 'amd64'),
    'aarch64-apple-darwin': ('darwin', 'arm64'),
}


def run(*args, cwd=ROOT, env=None):
    subprocess.run(args, cwd=cwd, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', action='append', required=True, choices=[*TARGETS, 'host'])
    parser.add_argument('--source-archive', type=Path, help='verified local archive for offline builds')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/croc-bundle')
    args = parser.parse_args()
    if 'host' in args.target:
        host = next(line.split(': ', 1)[1] for line in
            subprocess.check_output(['rustc', '-vV'], text=True).splitlines() if line.startswith('host: '))
        args.target = [host if target == 'host' else target for target in args.target]
        if host not in TARGETS:
            raise SystemExit('Unsupported Rust host: ' + host)
    work = ROOT / 'target/croc-build'
    work.mkdir(parents=True, exist_ok=True)
    archive = args.source_archive or work / (RECIPE['upstream_tag'] + '.zip')
    if not archive.exists():
        url = 'https://codeload.github.com/schollz/croc/zip/refs/tags/' + RECIPE['upstream_tag']
        with urllib.request.urlopen(url, timeout=120) as response:
            archive.write_bytes(response.read())
    source_bytes = archive.read_bytes()
    if hashlib.sha256(source_bytes).hexdigest() != RECIPE['source_sha256']:
        raise SystemExit('Croc source SHA-256 mismatch; refusing to build')
    with tempfile.TemporaryDirectory(prefix='source-', dir=work) as directory:
        destination = Path(directory).resolve()
        with zipfile.ZipFile(archive) as package:
            for member in package.infolist():
                if not (destination / member.filename).resolve().is_relative_to(destination):
                    raise SystemExit('Unsafe source archive entry')
            package.extractall(destination)
        source = destination / ('croc-' + RECIPE['upstream_tag'].removeprefix('v'))
        # Applying relative to the workspace root also works when Git discovers
        # the parent DataBeam repository around this downloaded source tree.
        run('git', '-c', 'safe.directory=' + ROOT.as_posix(), 'apply',
            '--directory=' + source.relative_to(ROOT).as_posix(),
            str(ROOT / 'patches' / RECIPE['patch']))
        test_env = dict(os.environ)
        test_env['GOTOOLCHAIN'] = 'go' + RECIPE['go_version']
        for key in ('GOOS', 'GOARCH', 'CROC_SECRET', 'CROC_RELAY', 'CROC_RELAY6'):
            test_env.pop(key, None)
        run('go', 'test', './src/croc', '-run', 'Test(LocalProbeResponse|DataBeamRoute)', '-count=1',
            '-timeout', '60s', cwd=source, env=test_env)
        for target in args.target:
            goos, goarch = TARGETS[target]
            output = args.output.resolve() / target
            output.mkdir(parents=True, exist_ok=True)
            binary = destination / ('croc.exe' if goos == 'windows' else 'croc')
            env = dict(test_env, GOOS=goos, GOARCH=goarch, CGO_ENABLED='0')
            flags = '-s -w -X github.com/schollz/croc/v11/src/cli.Version=' + RECIPE['engine_version']
            run('go', 'build', '-trimpath', '-buildvcs=false', '-ldflags', flags,
                '-o', str(binary), '.', cwd=source, env=env)
            data = binary.read_bytes()
            payload = gzip.compress(data, compresslevel=9, mtime=0)
            metadata = dict(version=RECIPE['engine_version'], target=target,
                patch_sha256=hashlib.sha256((ROOT / 'patches' / RECIPE['patch']).read_bytes()).hexdigest(),
                binary_sha256=hashlib.sha256(data).hexdigest(),
                gzip_sha256=hashlib.sha256(payload).hexdigest())
            # Publish metadata last; build.rs verifies both identity and bytes.
            (output / 'croc.gz').write_bytes(payload)
            (output / 'manifest.json').write_text(json.dumps(metadata, indent=2) + '\n')
            print('Prepared', target, RECIPE['engine_version'], flush=True)


if __name__ == '__main__':
    main()
