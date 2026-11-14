# Croc v11.6.5 evaluation — upgrade deferred

Release decision (2026-11-15): at the user's request, DataBeam 0.8.0 retains Croc 11.5.5 on desktop as well as Android. The observations below describe the evaluated 11.6.5 build, not the shipping engine. The desktop recipe now reuses the already validated 11.5.5 GUI telemetry patch from Android, without Android's `croc_no_tailcat` build tag. Both platforms retain selection-wide progress and verification reporting.

The separately reported desktop EasySendMe `open receive root: open C:\WINDOWS\TEMP: Access is denied` failure concerns the ticket handoff's working directory. The same error was reproduced locally with Croc 11.5.5 and a receive working directory of `C:\Windows\Temp`; it is not specific to 11.6.5. The desktop ticket receiver now owns a private temporary child directory until Croc exits, then removes it. The file destination setting is independent. The real EasySendMe regression now uses that same ticket receiver, instead of supplying an artificial handoff folder that masked the UI path.

Upstream latest was verified at https://github.com/schollz/croc/releases/tag/v11.6.5 on 2026-11-14. The official tagged source ZIP is pinned to SHA-256 `4685fdf9b3f57b47c50efd0a9136b5728ad445461d2500bd1309c1d6f16a5349`; Go 1.27.3 follows upstream go.mod.

The v11.5.5 runtime modifications for Windows text temp paths and probe fallback are now upstream. Their old hunks are omitted rather than layered over the new discovery budget. A regression verifies that a delayed PAKE reply is fully consumed before returning the discovery-budget fallback, leaving the next protocol frame intact.

DataBeam retains local GUI preparation/rate reporting and adds selection-wide payload bytes, completed-file counts and rate. Resume/skipped bytes affect progress without inflating rate. Both legacy chunks and negotiated bundles report original-file bytes. No DataBeam output wrapper is added for Croc.

The evaluated build used patches/croc-v11.6.5-databeam.patch. scripts/prepare_croc.py verifies archive/patch/payload identity, runs regressions and creates the desktop engine. Android has an independent recipe and stays on its validated v11.5.5 base with progress telemetry and croc_no_tailcat. A real v11.6.5 Android folder receive failed in the new bundle capability probe with linkat permission denied. Final build and device results are recorded in android/VALIDATION.md.
