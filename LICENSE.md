# License

## What this repository is

An **unofficial** port of OpenXOPS 1.080, the
open-source remake of *X operations*. It is not the official OpenXOPS, it is not
affiliated with the OpenXOPS Project, and it is not distributed under the
OpenXOPS name.

* It is written **from the OpenXOPS 1.080 C++ sources**, which are the reference
  for every question about how the game behaves: the game above the platform layer
  is a translation of them, file for file and function for function, and the
  comments in the code name the upstream file and line each piece came from.
* **Those sources are not redistributed here, and neither is any other part of
  OpenXOPS.** Get them from the OpenXOPS Project at <https://openxops.net/>.
* `crates/` and `missions/` are this port's own code. The platform layer -- the
  window, the input, the audio, the renderer -- is new, because the APIs the
  original used no longer exist.

## OpenXOPS: 3-clause BSD

The upstream terms are published at
<https://openxops.net/developer-original.php> (Japanese, authoritative). OpenXOPS
is licensed under the 3-clause BSD license, reproduced here verbatim:

```
OpenXOPS  Copyright (c) 2014-2022, OpenXOPS Project / [-_-;](mikan) All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

 * Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.
 * Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
 * Neither the name of the OpenXOPS Project nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL OpenXOPS Project BE LIABLE FOR ANY
DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

That copyright notice, those conditions and that disclaimer apply to this port,
because this port is a translation of the work they cover: it is a derivative work
of OpenXOPS 1.080, and the notice is retained for exactly that reason. It appears
in `LICENSE`, which is the licence this repository is under, with this port's own
copyright line beside it. `LICENSE` is the standard text and nothing else, which is
what lets a tool recognise it; the explanation, the third-party notices and the
rules that are not part of the licence text itself are here instead.

## This port

`crates/`, `missions/`, `LICENSE`, `LICENSE.md` and the rest of this repository are
released under the same 3-clause BSD terms, copyright (c) 2026 BlaCkinkGJ and
contributors of this port. The full text is in `LICENSE`; this file is the longer
notice it points at.

## Game data is not included

This repository ships **no** game data: `data/` is git-ignored and the
build is built without it. Neither is any of it in the repository's history --
no graphic, sound or mission file of the original has ever been committed, and
`.gitignore` keeps the whole `data/` directory out. The graphics data of the original game is a separate
work, and the upstream terms forbid redistributing it:

* Do not bundle the *X operations* graphic data with a distribution of this port.
* Users are expected to obtain *X operations* 0.96 themselves (freeware, by
  nine-two, distributed through <https://www.vector.co.jp/soft/win95/game/se284467.html>)
  and copy its `data` folder into `data/`.

The graphics data of *X operations* is copyright TENKUU, and the game itself is
by nine-two. The upstream project is working from permission granted per project,
not from a blanket license, so treat that data as not redistributable.

## Third-party components named by the upstream sources

The upstream sources reference two files by nine-two, neither of which this
repository redistributes. This port replaces both of them.

```
config4xops05.exe  (c) 2003-2006 nine-two
ezds.dll           (c) 2003-2006 nine-two
```

Those files may only be used by OpenXOPS and its derived projects, and their
copyright notice has to be shown in the documentation of any such project.

The upstream 1.080 source archive additionally bundles libjpeg, libpng and zlib
under their own licenses. Those copies are not part of this repository either, and
nothing here links or reads them: this port decodes JPEG and PNG in Rust, through
the `jpeg-decoder` and `png` crates.

## Naming

Upstream asks that a modified build uses its own name rather than the OpenXOPS or
*X operations* names, so this project goes by `xops` -- the crates, the binary and
its own title -- in a repository named `xops-rs`, and describes itself as an
unofficial port. It must not be presented as the
official OpenXOPS, nor as a product of the OpenXOPS Project, of team MITEI, or of
the authors of *X operations*.
