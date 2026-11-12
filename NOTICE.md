# Notices

mcbench-lite's own code is under the MIT licence in `LICENSE`. The files and
material below are not: each stays under its owner's terms.

mcbench-lite is not an official Minecraft product. It is not approved by or
associated with Mojang or Microsoft.

## Minecraft

Minecraft is a trademark of Mojang Synergies AB. Its code and assets belong
to Mojang Studios and Microsoft under the Minecraft End User Licence
Agreement (https://www.minecraft.net/eula).

This repository contains no Minecraft code, jars, textures, sounds or data
tables. `core/` is a Rust engine written to behave as Minecraft's
block-update code does, checked against the game.

When the app first starts, on the machine it runs on, it downloads the
Minecraft server (through Paper) and the Minecraft client jar from Mojang's
servers and accepts the Minecraft End User Licence Agreement on that
machine's behalf. It then reads the block, loot and data-fixer tables the
engine needs from that server jar into `core/minecraft/`, which git ignores,
and makes the 3D viewer's textures from that client jar. None of those files
may be committed or shared. A build shared with the app's Share feature puts
a copy of those textures on the user's own GitHub Pages site, as a BlueMap
map does.

`server/native/1.21.10/` holds a native-image profile and reachability
metadata recorded while running the Minecraft server. Only the parity tools
that build the Java reference engine use it; the app does not.

## BlueMap

The 3D viewer is BlueMap's web app (https://github.com/BlueMap-Minecraft/BlueMap),
downloaded when the app first starts and copied into shared builds.
`renderer/` is a Rust port of BlueMap 5.22's high-resolution tile mesher.
BlueMap is Copyright (c) Blue (Lukas Rieger) and contributors, under the MIT
licence:

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

## Java's sine and cosine

Minecraft's results depend on how Java computes `Math.sin` and `Math.cos`, so
the engine computes them the same way.

`core/src/jmath.rs` ports FdLibm's sine and cosine, as `java.lang.StrictMath`
uses them:

> Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
> Developed at SunSoft, a Sun Microsystems, Inc. business.
> Permission to use, copy, modify, and distribute this software is freely
> granted, provided that this notice is preserved.

`core/src/jmath/intel.rs` is a transliteration of Intel's LIBM sine and
cosine as OpenJDK's HotSpot and GraalVM emit them on x86-64 (Copyright (c)
2016, Intel Corporation; Copyright (c) Oracle and/or its affiliates). That
code is under the GNU General Public License, version 2, and so is this
file: it is not under the MIT licence.

## Logos

The AI providers' logos in `web/icons/` are their owners' trademarks, shown
to say which provider a model comes from.
