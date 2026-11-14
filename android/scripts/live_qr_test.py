from pathlib import Path
import subprocess,hashlib,json,time,sys
sys.stdout.reconfigure(encoding='utf-8')
R=Path(__file__).resolve().parents[2];W=R/'android/.tools/smoke';peer=R/'android/native/target/x86_64-pc-windows-msvc/debug/examples/peer.exe';fixture=W/'fixture.bin'
log=(W/'live-qr-peer.log').open('w');p=subprocess.Popen([str(peer),'send',str(fixture),str(W/'live-qr-cache')],stdout=subprocess.PIPE,stderr=log,text=True)
try:
 ticket=p.stdout.readline().strip().removeprefix('TICKET=');assert ticket.startswith('blob')
 (W/'live-qr-ticket.txt').write_text(ticket)
 jars=list((R/'android/.tools/gradle/caches/modules-2/files-2.1/com.google.zxing/core').rglob('core-*.jar'))
 subprocess.run(['X:/as/jbr/bin/java.exe','--class-path',str(jars[0]),str(W/'QrFixture.java'),str(W/'sendme-live-qr.png'),str(W/'live-qr-ticket.txt')],check=True)
 print('Live Sendme QR ready. Expected SHA256',hashlib.sha256(fixture.read_bytes()).hexdigest(),flush=True)
 p.wait()
finally:
 if p.poll() is None:p.terminate()
 log.close()
