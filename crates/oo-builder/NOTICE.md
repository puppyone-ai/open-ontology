# oo-builder third-party notices

First-party code in this crate is under the Apache License, Version 2.0, as stated in the repository `LICENSE` and `NOTICE`.

## unicode-normalization 0.1.25 and tinyvec 1.13.3

`oo-builder` calls `unicode-normalization` 0.1.25 for canonical NFC. The crates.io checksum is `5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8`. Its SPDX license is `MIT OR Apache-2.0`. Its only dependency is `tinyvec` 1.13.3, checksum `fd3ca314f692efd6c868f8408f53fe444634a845f96c028b97d35f6a1f79f0ee`, SPDX `Zlib OR Apache-2.0 OR MIT`, feature `alloc`.

`unicode-normalization` 0.1.25 reports `UNICODE_VERSION` `(17, 0, 0)`. The generated tables in that crate are Unicode data. The crate `COPYRIGHT` does not include the Unicode copyright and permission notice. The notice below covers that data and the Unicode files copied under `tests/unicode/`.

## Unicode data

Copied files and their URLs are listed in `tests/unicode/PROVENANCE.md`. The verbatim permission text retrieved from https://www.unicode.org/license.txt (SHA-256 `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96`) is `tests/unicode/LICENSE.txt`:

```
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2026 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```

`ERRATUM.md` records the earlier embedded Unicode 16 tables and the unsupported generator claim in their header.
