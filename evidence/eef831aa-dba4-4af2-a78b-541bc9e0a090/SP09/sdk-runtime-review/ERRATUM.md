# Erratum — r-efi AUTHORS is a MIT body

This file is new. It does not edit a078, a084, or a089.

## The false sentence

a089 `HANDOFF.md` line 16 says:

> `AUTHORS` is not a MIT license body.

That sentence is false. The same line says a078 already corrected the record. a078 did. The delivery commit still carries the false sentence in the a089 handoff, and the later stamp `fc877c482a61729ea46e779a674a46f8f72c80c5` edits that handoff and `REPORT.md` only. This erratum leaves both of those files as they are.

## What a084 actually says

a084 `review.md` lines 134–136 say `AUTHORS` is not a license locator. The basename rule in `supply-chain/policy.md` §2 is `LICENSE`, `LICENCE`, `COPYING`, or `NOTICE`. a084 does not say the MIT grant is missing. `SUP-REFI-OR-POLICY` stays pending on that basename rule and on `LGPL-2.1-or-later`. This erratum does not close either point, does not add a CDLA allow, and does not change `ci/deny.toml`, `Cargo.toml`, or `Cargo.lock`.

## Body and hash, recomputed from the delivery commit

Delivery commit `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`.

| file | git blob | bytes | sha256 |
| --- | --- | --- | --- |
| `contracts/supply-chain/isc/bodies/r-efi-6.0.0/AUTHORS` | `79c8b7c4a1dabcfbc718b7fe6d43b1892f9850cd` | 3733 | `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111` |
| `contracts/supply-chain/isc/bodies/r-efi-5.3.0/AUTHORS` | delivery tree | 3693 | `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7` |

Both hashes match a078. The 6.0.0 body below is the delivery blob. `AUTHORS-MIT` is the MIT grant: it begins `Permission is hereby granted, free of charge` and includes `The above copyright notice and this permission notice shall be included`. The copyright lines are Red Hat, Inc. (2017–2023), Microsoft Corporation (2019–2023), and David Rheinsberg (2022–2023). `AUTHORS-ASL` points at `http://www.apache.org/licenses/LICENSE-2.0`. `AUTHORS-LGPL` points at `http://www.gnu.org/licenses/`. Those two sections are pointers. The crate still has no file named `LICENSE`, `LICENCE`, `COPYING`, or `NOTICE`.

```
LICENSE:
        This project is triple-licensed under the MIT License, the Apache
        License, Version 2.0, and the GNU Lesser General Public License,
        Version 2.1+.

AUTHORS-MIT:
        Permission is hereby granted, free of charge, to any person obtaining a
        copy of this software and associated documentation files (the
        "Software"), to deal in the Software without restriction, including
        without limitation the rights to use, copy, modify, merge, publish,
        distribute, sublicense, and/or sell copies of the Software, and to
        permit persons to whom the Software is furnished to do so, subject to
        the following conditions:

        The above copyright notice and this permission notice shall be included
        in all copies or substantial portions of the Software.

        THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
        OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
        MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
        IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
        CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
        TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
        SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

AUTHORS-ASL:
        Licensed under the Apache License, Version 2.0 (the "License");
        you may not use this file except in compliance with the License.
        You may obtain a copy of the License at

                http://www.apache.org/licenses/LICENSE-2.0

        Unless required by applicable law or agreed to in writing, software
        distributed under the License is distributed on an "AS IS" BASIS,
        WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
        See the License for the specific language governing permissions and
        limitations under the License.

AUTHORS-LGPL:
        This program is free software; you can redistribute it and/or modify it
        under the terms of the GNU Lesser General Public License as published
        by the Free Software Foundation; either version 2.1 of the License, or
        (at your option) any later version.

        This program is distributed in the hope that it will be useful, but
        WITHOUT ANY WARRANTY; without even the implied warranty of
        MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
        Lesser General Public License for more details.

        You should have received a copy of the GNU Lesser General Public License
        along with this program; If not, see <http://www.gnu.org/licenses/>.

COPYRIGHT: (ordered alphabetically)
        Copyright (C) 2017-2023 Red Hat, Inc.
        Copyright (C) 2019-2023 Microsoft Corporation
        Copyright (C) 2022-2023 David Rheinsberg

AUTHORS: (ordered alphabetically)
        Alan Egerton <eggyal@gmail.com>
        Alex James <theracermaster@gmail.com>
        Ayush Singh <ayushsingh1325@gmail.com>
        Boris-Chengbiao Zhou <bobo1239@web.de>
        Bret Barkelew <bret@corthon.com>
        Christopher Zurcher <christopher.zurcher@microsoft.com>
        David Rheinsberg <david@readahead.eu>
        Dmitry Mostovenko <trueberserker@gmail.com>
        Hiroki Tokunaga <tokusan441@gmail.com>
        Joe Richey <joerichey@google.com>
        John Schock <joschock@microsoft.com>
        Michael Kubacki <michael.kubacki@microsoft.com>
        Oliver Smith-Denny <osde@microsoft.com>
        Richard Wiedenhöft <richard@wiedenhoeft.xyz>
        Rob Bradford <robert.bradford@intel.com>, <rbradford@rivosinc.com>
        Tom Gundersen <teg@jklm.no>
        Trevor Gross <tmgross@umich.edu>
```
