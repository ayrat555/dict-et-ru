<p align="center">
  <img src="assets/kobo-et-ru-logo.png" alt="Open book in the blue, black, and white colors of the Estonian flag" width="420">
</p>

# Estonian–Russian dictionary (Kobo, Kindle, StarDict)

Kobo-, Kindle-, and StarDict-compatible Estonian→Russian dictionaries built from the official EKI Estonian–Russian Dictionary, with extra inflection lookup so declined and conjugated forms resolve in books.

## Install

### Kobo

Copy [`out/dicthtml-et-ru.zip`](out/dicthtml-et-ru.zip) to the reader:

```text
KOBOeReader/.kobo/custom-dict/dicthtml-et-ru.zip
```

Eject the device and pick the Estonian–Russian dictionary when looking up a word.

Firmware 4.24+ loads `dicthtml-LOCALE.zip` from `.kobo/custom-dict`. Older firmware may need the extra-dictionaries patch; see [dictutil install notes](https://pgaskin.net/dictutil/dicthtml/install.html).

### Kindle

Unzip [`out/dict-et-ru.mobi.zip`](out/dict-et-ru.mobi.zip) and copy `dict-et-ru.mobi` to the Kindle `documents` folder, then set it as the Estonian dictionary:

```text
Settings → Language & Dictionaries → Dictionaries
```

### StarDict / KOReader / GoldenDict

Unzip [`out/stardict-et-ru.zip`](out/stardict-et-ru.zip) and copy the `stardict-et-ru` folder into the reader’s StarDict directory.

## Contents

| File | Role |
|------|------|
| `out/dicthtml-et-ru.zip` | Ready-to-install Kobo dictionary |
| `out/dict-et-ru.mobi.zip` | Kindle dictionary (unzip, then copy the `.mobi`) |
| `out/stardict-et-ru.zip` | StarDict bundle for KOReader, GoldenDict, sdcv (unzip first) |

How the files are produced: [docs/build.md](docs/build.md).

## License

[CC BY-SA 4.0](LICENSE)

This is a derived work. Credit:

1. **Eesti Keele Instituut (EKI)**, *Eesti-vene sõnaraamat* (EVS), [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). XML: [evs_EKI_CCBY40.xml](https://arhiiv.eki.ee/litsents/idkaart/evs/evs_EKI_CCBY40.xml).
2. **EKI / Ekilex** inflected forms via [Estonian-Wordlist-Enriched-Ekilex](https://github.com/KristjanPikhof/Estonian-Wordlist-Enriched-Ekilex) ([CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/)).

Changes: conversion to Kobo dicthtml, Kindle MOBI, and StarDict, stress-mark reformatting, trimmed examples, and lookup-key expansion from inflected forms.
