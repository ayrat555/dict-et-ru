# Estonian–Russian dictionary (Kobo and Kindle)

Kobo-, Kindle-, and StarDict-compatible Estonian→Russian dictionaries built from the official EKI Estonian–Russian Dictionary, with extra inflection lookup so declined and conjugated forms resolve in books.

## Install

### Kobo

Copy [`dicthtml-et-ru.zip`](dicthtml-et-ru.zip) to the reader:

```text
KOBOeReader/.kobo/custom-dict/dicthtml-et-ru.zip
```

Eject the device and pick the Estonian–Russian dictionary when looking up a word.

Firmware 4.24+ loads `dicthtml-LOCALE.zip` from `.kobo/custom-dict`. Older firmware may need the extra-dictionaries patch; see [dictutil install notes](https://pgaskin.net/dictutil/dicthtml/install.html).

### Kindle

Copy [`dict-et-ru.mobi`](dict-et-ru.mobi) to the Kindle `documents` folder, then set it as the Estonian dictionary:

```text
Settings → Language & Dictionaries → Dictionaries
```

### StarDict / KOReader / GoldenDict

Copy the [`stardict-et-ru`](stardict-et-ru) folder (`.ifo`, `.idx`, `.dict`, `.syn`) into the reader’s StarDict directory.

## Contents

| File | Role |
|------|------|
| `dicthtml-et-ru.zip` | Ready-to-install Kobo dictionary |
| `dict-et-ru.mobi` | Ready-to-install Kindle dictionary |
| `stardict-et-ru/` | StarDict bundle for KOReader, GoldenDict, sdcv |
| `data/est_inflected_forms.tsv` | Inflected forms used as extra lookup keys |
| `scripts/build.sh` | Download EVS XML, rebuild the Kobo zip |
| `scripts/build_est_ru_df.py` | EVS + inflections → dictutil `.df` |
| `scripts/build_kindle.sh` | Kobo zip → Kindle MOBI |
| `scripts/build_kindle.py` | Kobo zip → Kindle OPF/XHTML |
| `scripts/build_stardict.sh` | Kobo zip / Kindle OPF → StarDict |

EVS XML is not included (about 85 MB). The build script downloads it from EKI.

## Rebuild

Kobo:

```bash
./scripts/build.sh
```

Kindle or StarDict (both read the Kobo zip):

```bash
./scripts/build_kindle.sh
./scripts/build_stardict.sh
```

Kobo build needs Python 3.10+, `curl`, and either Go or a [dictgen](https://github.com/pgaskin/dictutil) binary. On Apple Silicon, Go is required to build dictgen (there is no official arm64 binary).

Kindle and StarDict builds need the Kobo zip plus [kindling-cli](https://github.com/ciscoriordan/kindling) (the scripts download it). StarDict reuses the Kindle OPF if it is already in `dist/kindle`.

## License

[CC BY-SA 4.0](LICENSE)

This is a derived work. Credit:

1. **Eesti Keele Instituut (EKI)**, *Eesti-vene sõnaraamat* (EVS), [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). XML: [evs_EKI_CCBY40.xml](https://arhiiv.eki.ee/litsents/idkaart/evs/evs_EKI_CCBY40.xml).
2. **EKI / Ekilex** inflected forms via [Estonian-Wordlist-Enriched-Ekilex](https://github.com/KristjanPikhof/Estonian-Wordlist-Enriched-Ekilex) ([CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/)).

Changes: conversion to Kobo dicthtml, Kindle MOBI, and StarDict, stress-mark reformatting, trimmed examples, and lookup-key expansion from inflected forms.
