"""Exercise revised Croc resumes on a connected Android phone with generated files."""
import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import subprocess
import time
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path
from urllib.parse import quote, unquote

ROOT = Path(__file__).resolve().parents[2]
ADB = 'X:/asproj/platform-tools/adb.exe'
APP = 'com.vinaywinai.databeam'
CROC = ROOT / 'target/croc-bundle/x86_64-pc-windows-msvc/croc.exe'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--fixture-folder', action='store_true', help='Use the test-only custom document tree')
    parser.add_argument('--recover', action='store_true', help='Restart from a fixture with an approved update interrupted during reset')
    parser.add_argument('--unwrap', action='store_true', help='Save the verified files directly in the destination')
    args = parser.parse_args()
    session = str(uuid.uuid4())
    tree = f'content://{APP}.test.revisionfixture/tree/revision-{session}' if args.fixture_folder else ''
    work = ROOT / 'android/.tools/smoke/croc-revision' / session
    work.mkdir(parents=True)
    print('Test artifacts:', work, flush=True)
    processes, handles, records, reversed_ports = [], [], [], []
    relay_ports = (39569, 39570)
    relay = f'127.0.0.1:{relay_ports[0]}'

    def adb(*params, input=None, check=True):
        result = subprocess.run([ADB, *params], input=input, capture_output=True, check=check, timeout=25)
        return result.stdout

    def private(*params, **options):
        return adb('shell', 'run-as', APP, *params, **options)

    def state():
        raw = adb('exec-out', 'run-as', APP, 'cat', 'shared_prefs/transfer.xml', check=False)
        node = ET.fromstring(raw).find("string[@name='state']") if raw.startswith(b'<?xml') else None
        return json.loads(node.text) if node is not None else {}

    def stop(proc):
        if proc.poll() is None:
            proc.terminate()
        proc.wait(timeout=10)

    def spawn(command, label, env=None):
        output = (work / (label + '.log')).open('wb')
        handles.append(output)
        proc = subprocess.Popen(list(map(str, command)), stdout=output, stderr=subprocess.STDOUT, env=env,
                                creationflags=0x08000000)
        processes.append(proc)
        return proc

    def phone(config, label):
        encoded = base64.b64encode(json.dumps(config).encode()).decode()
        return spawn([ADB, 'shell', 'am', 'instrument', '-w', '-e', 'config', encoded,
                      f'{APP}.test/{APP}.SmokeInstrumentation'], label)

    def sender(files, code, label):
        env = dict(os.environ, CROC_SECRET=code, DATABEAM_CROC_FILE_DIGEST='1', CROC_NO_UPDATE_CHECK='1')
        return spawn([CROC, '--yes', '--ignore-stdin', '--relay', relay, '--throttleUpload', '6m',
                      'send', '--no-local', '--', *files], label, env)

    def wait_state(predicate, seconds, code=None):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            value = state()
            if value.get('id') == session and (code is None or value.get('code') == code):
                if predicate(value):
                    return value
                if value.get('status') == 'interrupted':
                    raise RuntimeError(value.get('message'))
            time.sleep(.15)
        raise TimeoutError(f'No matching phone state within {seconds} seconds')

    assert state().get('status') not in ('running', 'stopping'), 'A transfer is already active'
    saved = {name: adb('exec-out', 'run-as', APP, 'cat', f'shared_prefs/{name}', check=False)
             for name in ('settings.xml', 'transfer.xml')}
    tag = session[:8]
    old_dir, new_dir = work / 'initial', work / 'updated'
    old_dir.mkdir()
    new_dir.mkdir()
    original = 'original-' + tag
    keep_name, edit_name, remove_name, add_name = (
        f'rev-{tag}-keep.bin', f'rev-{tag}-edit.bin', f'rev-{tag}-remove.bin', f'rev-{tag}-renamed.bin')
    keep = old_dir / keep_name
    keep.write_bytes(os.urandom(48 * 1024 * 1024))
    (old_dir / edit_name).write_bytes(os.urandom(128 * 1024))
    (new_dir / edit_name).write_bytes(os.urandom(128 * 1024))
    removed_payload = os.urandom(64 * 1024)
    (old_dir / remove_name).write_bytes(removed_payload)
    (new_dir / add_name).write_bytes(removed_payload)
    base = dict(id=session, mode='Croc', action='receive', code=original, relay=relay,
                saveFolder=tree, unwrapCroc=args.unwrap, testTimeoutSeconds=95)
    collision = keep_name if args.unwrap else keep_name[:20] + '_and_more_databeam'
    download_conflict = f'/sdcard/Download/DataBeam/{keep_name}'
    if tree:
        base['revisionFixtureCollision'] = collision
    try:
        if args.unwrap and not tree:
            adb('shell', 'mkdir', '-p', '/sdcard/Download/DataBeam')
            adb('shell', 'mkdir', download_conflict)
            adb('shell', f'printf existing > {download_conflict}/sentinel')
        relay_binary = work / 'croc-relay.exe'
        shutil.copyfile(CROC, relay_binary)
        relay_process = spawn([relay_binary, 'relay', '--host', '127.0.0.1', '--ports',
                               ','.join(map(str, relay_ports))], 'relay')
        time.sleep(1)
        assert relay_process.poll() is None, 'The local relay did not start'
        for port in relay_ports:
            adb('reverse', '--no-rebind', f'tcp:{port}', f'tcp:{port}')
            reversed_ports.append(port)
        first = sender([old_dir / remove_name, old_dir / edit_name, keep], original, 'initial-sender')
        receiver = phone(base, 'initial-phone')
        wait_state(lambda s: s.get('done', 0) >= 9 * 1024 * 1024, 55)
        adb('shell', 'am', 'force-stop', APP)
        receiver.wait(timeout=12)
        stop(first)
        frozen = state()
        records.append(frozen)
        receipt_name = f'files/croc-resume/{session}.json'
        receipt = private('cat', receipt_name)
        assert bool(frozen.get('crocDirect')) == (not tree), 'Unexpected storage path'
        config = dict(frozen, reviewCrocChanges=True, testTimeoutSeconds=95)
        config.pop('revisionFixtureCollision', None)
        updated_payload = [keep, new_dir / edit_name, new_dir / add_name]
        decisions = ('recover',) if args.recover else ('disconnect', 'keep', 'apply')
        for decision in decisions:
            revised = f'revised-{decision}-{tag}'
            if decision == 'recover':
                previous = json.loads(receipt)
                next_offer = dict(previous, entries=sorted([
                    dict(path=file.name, kind='file', size=file.stat().st_size,
                         digest=base64.b64encode(hashlib.sha256(file.read_bytes()).digest()).decode())
                    for file in updated_payload
                ], key=lambda entry: entry['path']))
                next_offer['total'] = sum(entry['size'] for entry in next_offer['entries'])
                update = dict(id=uuid.uuid4().hex, previous=previous, next=next_offer,
                              files=sorted([edit_name, remove_name]), directories=[])
                private('tee', receipt_name+'.revision', input=json.dumps(update).encode())
                if not tree:
                    pending = frozen['crocPending'][edit_name]
                    assert re.fullmatch(r'content://media/(external|external_primary)/downloads/\d+', pending)
                    adb('shell', 'content', 'delete', '--uri', pending)
            offerer = sender(updated_payload, revised, f'{decision}-sender')
            response = decision if decision in ('keep', 'apply') else ''
            receiver = phone(dict(config, code=revised, reviewDecision=response), f'{decision}-phone')
            if decision == 'disconnect':
                wait_state(lambda s: bool(s.get('crocReview')), 40, revised)
                stop(offerer)
            receiver.wait(timeout=85)
            report = (work / f'{decision}-phone.log').read_text(encoding='utf-8', errors='replace')
            if response:
                reviewed = json.loads(re.search(r'revision_review=(\{[^\r\n]+\})', report)[1])
                assert reviewed['replaced'] == [edit_name] and reviewed['added'] == [add_name]
                assert reviewed['removed'] == [remove_name]
            current = state()
            records.append(current)
            stop(offerer)
            if decision in ('disconnect', 'keep'):
                assert 'The saved partial transfer was kept.' in current.get('message', ''), current.get('message')
                assert private('cat', receipt_name) == receipt
                assert current.get('crocPending') == frozen.get('crocPending')
                print(f'PASS {decision} preserved the original transfer', flush=True)
            else:
                assert current.get('status') == 'complete', current.get('message')
                assert current.get('firstPayloadDone', 0) >= 8 * 1024 * 1024, 'Unchanged file restarted from zero'
                assert set(current['published']) == {keep_name, edit_name, add_name}
                for item in updated_payload:
                    digest = hashlib.sha256(item.read_bytes()).hexdigest()
                    assert f'download_hash_{item.name}={digest}' in report, 'Saved hash mismatch'
                if current.get('crocDirect'):
                    assert current['published'][keep_name] == frozen['crocPending'][keep_name]
                    assert current['published'][edit_name] != frozen['crocPending'][edit_name]
                    if args.unwrap:
                        target = current['crocMoveTargets'][keep_name]
                        assert target == keep_name.replace('.bin', ' (1).bin')
                        assert adb('shell', 'cat', download_conflict + '/sentinel') == b'existing'
                        final_size = adb('shell', f"stat -c %s '/sdcard/Download/DataBeam/{target}'")
                        assert int(final_size) == keep.stat().st_size
                        assert not adb('shell', 'ls', '-d', f"/sdcard/Download/DataBeam/{current['crocWrapper']}", check=False)
                if tree:
                    if args.unwrap:
                        target = current['crocMoveTargets'][keep_name]
                        assert target == keep_name.replace('.bin', ' (1).bin')
                        expected = f"revision-{session}/{target}"
                    else:
                        assert current['crocWrapper'] != collision
                        expected = f"revision-{session}/{current['crocWrapper']}/{keep_name}"
                    assert unquote(current['published'][keep_name]) == tree + '/document/' + expected
                    existing = quote(f'revision-{session}/{collision}/existing.txt', safe='')
                    assert adb('shell', 'content', 'read', '--uri', tree + '/document/' + existing) == b'existing destination'
                assert not private('cat', receipt_name, check=False)
                assert current.get('crocMoved', False) == args.unwrap
                print(f"PASS revised batch published, resumed at {current['firstPayloadDone']} bytes", flush=True)
    finally:
        for proc in reversed(processes):
            stop(proc)
        for port in reversed_ports:
            adb('reverse', '--remove', f'tcp:{port}', check=False)
        if args.unwrap and not tree:
            adb('shell', 'rm', '-f', download_conflict + '/sentinel', check=False)
            adb('shell', 'rmdir', download_conflict, check=False)
        records.append(state())
        adb('shell', 'am', 'force-stop', APP, check=False)
        for record in records:
            if record.get('id') != session:
                continue
            for uri in {*record.get('published', {}).values(), *record.get('crocPending', {}).values()}:
                if re.fullmatch(r'content://media/(external|external_primary)/downloads/\d+', uri):
                    adb('shell', 'content', 'delete', '--uri', uri, check=False)
            wrapper = record.get('crocWrapper', '')
            if wrapper.startswith(f'rev-{tag}-') and re.fullmatch(r'[a-z0-9_.-]+', wrapper):
                adb('shell', 'rmdir', '/sdcard/Download/DataBeam/' + wrapper, check=False)
        private('rm', '-f', *(f'files/croc-resume/{session}.json{suffix}' for suffix in
                               ('', '.review', '.decision', '.revision', '.review.tmp', '.revision.tmp', '.tmp')),
                f'files/transfer-records/{session}.json', check=False)
        partial = f'files/received/{session}'
        resolved = private('readlink', '-f', partial, check=False).decode().strip()
        parent = private('readlink', '-f', 'files/received', check=False).decode().strip()
        if resolved and parent:
            assert resolved == parent + '/' + session
            private('rm', '-rf', partial)
        if tree:
            fixture = f'cache/revision-{session}'
            absolute = adb('shell', 'run-as', APP + '.test', 'readlink', '-f', fixture, check=False).decode().strip()
            parent = adb('shell', 'run-as', APP + '.test', 'readlink', '-f', 'cache', check=False).decode().strip()
            if absolute and parent:
                assert absolute == parent + f'/revision-{session}'
                adb('shell', 'run-as', APP + '.test', 'rm', '-rf', fixture)
        for name, contents in saved.items():
            if contents:
                private('tee', f'shared_prefs/{name}', input=contents)
            else:
                private('rm', '-f', f'shared_prefs/{name}')
            private('rm', '-f', f'shared_prefs/{name}.bak', check=False)
        for handle in handles:
            handle.close()


if __name__ == '__main__':
    main()
