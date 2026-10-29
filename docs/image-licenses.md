# Image components and redistribution

The beweft source in this repository is MIT licensed; see `LICENSE`. The full
container image is an aggregation of independently licensed software and is
**not** entirely MIT licensed. In particular, the Alpine 3.22 FFmpeg package
includes GPL components and is identified by Alpine as `GPL-2.0-or-later AND
LGPL-2.1-or-later`. In the tested image, FFmpeg was built with `--enable-gpl`
and `--enable-version3`, and `ffmpeg -L` identifies its effective license as
GPL version 3 or later. Check the actual report for each rebuilt image. The app
invokes `ffprobe` as a separate program.

The image keeps the following in `/usr/share/beweft/licenses`:

- The beweft MIT license and GNU GPL version 2 and 3 / LGPL version 2.1 and 3
  texts.
- Rust and npm dependency inventories and license/notice files collected from
  the installed build dependencies. The inventories can include build-only
  packages; an empty or missing license field needs individual investigation.
- The Alpine installed-package database and package version list, FFmpeg build
  configuration and license report, and the two lockfiles. Use these to identify the exact binary
  components and their upstream packages.

These notices help an operator inspect a locally built image. **They are not a
complete offer of corresponding source and do not by themselves authorize
redistribution.** Before publishing or giving anyone an image, the distributor
must identify every included dependency and satisfy its license, including
providing the complete corresponding source and build/install scripts for the
exact distributed FFmpeg and other GPL/LGPL binaries, their Alpine packaging
patches, and any required third-party copyright and license notices. Preserve
those sources with each image revision; a moving package URL or this document
alone is not a source offer. Review the actual package inventory and fill any
missing license texts. Obtain legal advice if needed for a release.

References: [Alpine FFmpeg package and source metadata](https://pkgs.alpinelinux.org/package/v3.22/community/aarch64/ffmpeg),
[Alpine aports](https://gitlab.alpinelinux.org/alpine/aports), and the
[FFmpeg licensing guidance](https://ffmpeg.org/legal.html). The package page is
architecture-specific; consult the entry for the architecture actually built.
The Dockerfile selects the v3.22 repositories, whose package versions can change;
use the inventory captured in the image rather than assuming a fixed version.
