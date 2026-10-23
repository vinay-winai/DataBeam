# Third-Party Notices

This project uses and/or redistributes third-party software.  
The license notices below are provided to satisfy attribution requirements.

## 1) croc

- Project: [schollz/croc](https://github.com/schollz/croc)
- Copyright: (c) 2017-2025 Zack and contributors to `croc`
- License: MIT
- License text: [LICENSES/CROC-MIT.txt](LICENSES/CROC-MIT.txt), unchanged upstream notice
- Bundled engine: upstream v11.2.5 plus [DataBeam's receiver keepalive patch](patches/croc-v11.2.5-relay-keepalive.patch), labeled `11.2.5-databeam-keepalive`.

## 2) sendme (official upstream reference)

- Project: [n0-computer/sendme](https://github.com/n0-computer/sendme)
- Copyright: Contributors to `sendme`
- License: Apache-2.0 OR MIT
- License texts:
  - [LICENSES/Apache-2.0.txt](LICENSES/Apache-2.0.txt)
  - [LICENSES/MIT.txt](LICENSES/MIT.txt)

## 3) alt-sendme (vendored)

- Project: [tonyantony300/alt-sendme](https://github.com/tonyantony300/alt-sendme)
- Path in this repo: `third_party/sendme`
- License: AGPL-3.0
- License texts:
  - [LICENSES/AGPL-3.0.txt](LICENSES/AGPL-3.0.txt)
  - [LICENSE](LICENSE)

## 4) EFF Short Wordlist #1

- Creator: Electronic Frontier Foundation; wordlists by Joseph Bonneau
- Source: [EFF Dice-Generated Passphrases](https://www.eff.org/dice)
- Word-only copy: [croc v11.2.5](https://github.com/schollz/croc/blob/v11.2.5/src/codephrase/wordlists/eff-short-wordlist-1.txt), unchanged
- Path in this repo: `assets/wordlists/eff-short-wordlist-1.txt`
- License: [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/), under [EFF's copyright policy](https://www.eff.org/copyright)

## Notes

- The DataBeam project license text (AGPL-3.0) is stored at repository root in `LICENSE`.
- If distributed artifacts bundle third-party binaries, include this file and the referenced license texts with the distribution.
