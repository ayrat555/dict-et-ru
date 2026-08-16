# Kobo Estonian–Russian dictionary

Kobo-compatible Estonian→Russian dictionary (`dicthtml-et-ru.zip`) built from the official EKI Estonian–Russian Dictionary, with extra inflection lookup so declined and conjugated forms resolve in books.

## Install

Copy [`dicthtml-et-ru.zip`](dicthtml-et-ru.zip) to the reader:

```text
KOBOeReader/.kobo/custom-dict/dicthtml-et-ru.zip
```

Eject the device and pick the Estonian–Russian dictionary when looking up a word.

Firmware 4.24+ loads `dicthtml-LOCALE.zip` from `.kobo/custom-dict`. Older firmware may need the extra-dictionaries patch; see [dictutil install notes](https://pgaskin.net/dictutil/dicthtml/install.html).

## Contents

| File | Role |
|------|------|
| `dicthtml-et-ru.zip` | Ready-to-install Kobo dictionary (~65k headwords) |
| `data/est_inflected_forms.tsv` | Inflected forms used as extra lookup keys |
| `scripts/build.sh` | Download EVS XML, rebuild the zip |
| `scripts/build_est_ru_df.py` | EVS + inflections → dictutil `.df` |

EVS XML is not included (about 85 MB). The build script downloads it from EKI.

## Rebuild

```bash
./scripts/build.sh
```

Requires `python3`, `curl`, and either Go or a [dictgen](https://github.com/pgaskin/dictutil) binary.

## License

[CC BY-SA 4.0](LICENSE)

This is a derived work. Credit:

1. **Eesti Keele Instituut (EKI)**, *Eesti-vene sõnaraamat* (EVS), [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). XML: [evs_EKI_CCBY40.xml](https://arhiiv.eki.ee/litsents/idkaart/evs/evs_EKI_CCBY40.xml).
2. **EKI / Ekilex** inflected forms via [Estonian-Wordlist-Enriched-Ekilex](https://github.com/KristjanPikhof/Estonian-Wordlist-Enriched-Ekilex) ([CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/)).

Changes: conversion to Kobo dicthtml, stress-mark reformatting, trimmed examples, and lookup-key expansion from inflected forms.

