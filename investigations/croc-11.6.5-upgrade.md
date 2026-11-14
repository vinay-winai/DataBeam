# Croc v11.6.5 upgrade for DataBeam 0.8.0

Upstream latest was verified at https://github.com/schollz/croc/releases/tag/v11.6.5 on 2026-11-14. The official tagged source ZIP is pinned to SHA-256 `4685fdf9b3f57b47c50efd0a9136b5728ad445461d2500bd1309c1d6f16a5349`; Go 1.27.3 follows upstream go.mod.

The v11.5.5 runtime modifications for Windows text temp paths and probe fallback are now upstream. Their old hunks are omitted rather than layered over the new discovery budget. A regression verifies that a delayed PAKE reply is fully consumed before returning the discovery-budget fallback, leaving the next protocol frame intact.

DataBeam retains local GUI preparation/rate reporting and adds selection-wide payload bytes, completed-file counts and rate. Resume/skipped bytes affect progress without inflating rate. Both legacy chunks and negotiated bundles report original-file bytes. No DataBeam output wrapper is added for Croc.

The historical v11.5.5 patch remains unchanged for provenance; current builds use patches/croc-v11.6.5-databeam.patch. scripts/prepare_croc.py verifies archive/patch/payload identity, runs regressions and creates the desktop engine. Android has an independent recipe and stays on its validated v11.5.5 base with progress telemetry and croc_no_tailcat. A real v11.6.5 Android folder receive failed in the new bundle capability probe with linkat permission denied, so the desktop-only upgrade is not forced onto the phone. Final build and device results are recorded in android/VALIDATION.md.
