# Croc CLI / croc web DNS investigation

Tested on Windows x64 on 2026-10-23 with the official latest CLI, v11.5.5,
and the live site at https://getcroc.com/. All messages and files were synthetic.

## Finding

The reproduced failure is in the CLI's DNS lookup path, before the relay
connection or peer authentication. It is independent of the keepalive bug.
The web protocol worked in both directions when the same CLI used a
system-resolved relay IP.

`go version -m croc.exe` reports Go 1.27.2 and the official build tags
`netgo,osusergo`. `GODEBUG=netdns=2` confirms:

```text
confVal.netCgo = false  netGo = true
using the Go DNS resolver
```

This forces Go's DNS path instead of the Windows resolver. On this host,
Windows resolved the relay in roughly 20–50 ms, while Go's combined IPv4/IPv6
lookup reached the CLI's five-second dial deadline and returned `i/o timeout`.

A standalone Go 1.27.0 probe reproduced the difference without DataBeam or
Croc. It used `net.DefaultResolver` for the host path and a `PreferGo` resolver
with a logging UDP connection for the direct path. In one captured combined
lookup for `4.getcroc.com`, both A and AAAA questions were sent to `1.1.1.1:53`.
The empty, successful AAAA reply arrived; the A reply was not observed before
the five-second deadline. Separate A-only and AAAA-only lookups then completed
in about 110–120 ms, including a valid A answer. Other combined runs returned
the IPv4 answer only after the full five seconds.

Therefore the configured DNS servers are not universally unreachable. The
evidence isolates a problem with the direct Go/parallel DNS path in this
environment. It does **not** establish whether the missing/delayed response
originates in Go, a local network component, or an upstream network component.
Changing DNS/VPN settings is not a justified blanket diagnosis.

## Latest official CLI versus live web

Each independent transfer used a fresh code. A DNS-only failed attempt could
be retried against its still-waiting web peer because it had never reached
the relay. No partially transferred payload was replayed.

| Direction | CLI relay setting | Result |
| --- | --- | --- |
| Web → official 11.5.5 CLI | Automatic hostname, code mapped to `4.getcroc.com:9009` | DNS timed out after 5 seconds; no relay connection |
| Official 11.5.5 CLI → web | Automatic hostname, code mapped to `1.getcroc.com:9009` | Same DNS failure before authentication |
| Web → official 11.5.5 CLI | Matching relay IP from the Windows resolver | Actual 57-byte text printed by CLI; transfer completed |
| Official 11.5.5 CLI → web | Matching relay IP from the Windows resolver | Browser displayed the exact 45-byte text and reported verified; CLI exited successfully |

The CLI-to-web test required clicking the browser's **Display text** button.
An earlier unattended attempt reached the offered-text screen and timed out
waiting for that action; it was not a DNS or protocol failure. CLI receives
used an isolated destination and `--overwrite` for unattended synthetic tests.
One preliminary receive without that flag skipped the offered text; its
zero exit code and the browser's completion banner were not counted as proof
of delivery. Subsequent runs verified the actual payload.

## DataBeam mitigation and integration

DataBeam resolves public relay names with the operating-system resolver and
passes the resulting address to Croc. It preserves code-to-relay routing
and resolves names at runtime; no relay IP is hardcoded into the app.
An explicit user `CROC_RELAY` / `CROC_RELAY6` remains supported.

The current bundled `11.5.5-databeam` engine is built without forcing
`netgo`, allowing Go's normal platform resolver selection. On Windows and
macOS this also preserves the native resolver path. The receiver keepalive
handling remains separate from this build choice and the Rust DNS workaround.

The release recipe, source hash, patch hash, and per-target payload hashes
make the engine identity explicit. Automatic updates remain held so an
official binary cannot silently replace this tested engine.

## Proton VPN comparison

On 2026-10-23 the user enabled Proton VPN on the same Windows PC. The
ProTUN adapter was up, advertising IPv4 DNS `10.2.0.1` and IPv6 DNS
`2a07:b944::2:1`. The existing Ethernet adapter still advertised
`1.1.1.1` / `1.0.0.1`; no DNS or adapter settings were changed by the test.

The same standalone Go 1.27.0 probe was run twice against all four relay
names. All eight combined A/AAAA Go lookups returned the IPv4 relay address
successfully, in approximately 243–393 ms, rather than hitting the previous
five-second deadline. The probe observed both question types sent to
`[2a07:b944::2:1]:53` and both replies received. Separate A lookups also
succeeded. AAAA-only lookups received empty successful DNS replies and
reported no address, consistent with the absence of IPv6 relay records;
these were not timeouts.

An official, unmodified v11.5.5 CLI then sent a fresh synthetic 45-byte text
message to the live Croc web receiver using automatic relay selection,
without `--relay` or an IP override. Its log still confirmed the forced Go
resolver, and it connected to `2.getcroc.com:9009` and the data ports by
hostname. After accepting Display text, the browser showed the exact
payload `DataBeam CLI to web DNS regression 2026-10-23` and reported
45/45 bytes received and verified; the CLI exited 0. Total time included
waiting for browser interaction and is not a DNS timing measurement.

This comparison shows that the demonstrated timeout is environment/path
dependent and can disappear on this PC with Proton connected. It does not
isolate the DNS server as the sole cause: the VPN also changes routing,
address-family paths, and potentially filtering. It neither establishes
a general Croc DNS defect nor identifies a particular ISP, router, or
local software component as the root cause. The earlier VPN-off evidence
was from the prior investigation, not a new alternating on/off trial.
The independent relay keepalive/handshake defect remains a separate issue.

Local logs: `temp_croc_debug/proton-dns-probe.log`,
`temp_croc_debug/proton-dns-probe-repeat.log`, and
`temp_croc_debug/web-dns-send-1792765995878553200/cli.log`.

## Local evidence

The ignored `temp_croc_debug` directory retains the official binary, complete
CLI logs, a standalone DNS probe, and the web test launcher. Captured browser
snapshots are under ignored `.playwright-cli/`. They are diagnostic artifacts,
not part of the released app. The `netgo` behavior is documented in Go's
[`net` package](https://pkg.go.dev/net#hdr-Name_Resolution); Croc's direct dial
uses the DNS lookup inside its connection deadline in
[`src/comm/comm.go`](https://github.com/schollz/croc/blob/v11.5.5/src/comm/comm.go).
