# Public Croc relay recovery

When a Croc sender or EasySendMe ticket sender exits with a public-relay transport connection error before payload transfer, DataBeam displays "Auto troubleshooting Croc public relays..." and checks TCP reachability of the four public control endpoints concurrently. Diagnostics stop waiting after five seconds and respond to cancellation. Reachability is a local observation, not a claim of global relay health.

If a relay is reachable, the sender generates a strong fresh code whose SHA-256 maps to that relay, populates the code input, closes any stale QR popup, and retries once. The notice explains that the earlier code is temporarily failing due to a Croc public relay connection problem and that the replacement must be shared with the receiver. The advertised desktop share code is withheld until the replacement attempt is prepared.

If none are reachable, a Sendme suggestion and "Use Sendme instead" action appear. EasySendMe automatic retries do not loop on this terminal condition. The receiver never invents a new shared secret: it requests a replacement from the sender. Explicit custom relay settings are honored. Authentication, missing-room, compression and file errors do not trigger replacement, and started payloads are never automatically replayed. Existing DNS recovery still retries the same relay using the system resolver.

Desktop implementation: `src/backend.rs` and `src/main.rs`. Android implementation: `RelayRecovery.kt`, `TransferService.kt` and `MainActivity.kt`.

Validation on 2026-11-14:

- Full desktop suite: 135 passed, 6 ignored.
- Recovery tests cover replacement-code routing, all public relays down, receiver-secret preservation, cancellation, bounded retries, payload/authentication failures, stale-code/QR invalidation and EasySendMe retry suppression.
- Android unit tests cover bounded cancellation, relay probe failures, replacement routing and non-network error classification.
- Real Samsung Android 16 sender test: custom code `recovery-3` mapped to unreachable relay 3; troubleshooting appeared, a replacement mapping to a reachable public relay was generated, and the existing desktop Croc binary received the generated 1 MiB fixture with matching SHA-256. Test driver: `android/scripts/relay_recovery_smoke.py`.
- No server or protocol changes; replacement codes remain compatible with ordinary Croc clients.

Desktop Quick Actions also includes Send to browser: it opens a fresh Croc file-send view with a receiver instruction banner linking to https://getcroc.com.
