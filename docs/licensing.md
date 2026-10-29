# Licensing and upstream research

beweft's own source and original unicorn mark use the repository's MIT license.
The Buffer investigation informed behavior and usability; no Buffer code,
proprietary artwork, private content or raw authenticated captures are included.

Postiz and OpenPost were examined as AGPL projects, and Mixpost Lite as an MIT
project. No code was copied from those research projects. Their source-backed
comparison is in [connections](research/connections.md). None is a runtime or
publishing backend dependency.

Third-party dependencies retain their own licenses. Cargo and pnpm lockfiles
pin the dependency graph. The 2026-10-29 manifest inventory reported licenses
for all resolved Cargo packages, with permissive alternatives where an SPDX
expression provides a choice. Production JavaScript packages reported MIT,
Apache-2.0, ISC and 0BSD licenses. This inventory must be refreshed as packages
change; it does not replace their copyright/license notices.

Distribution packaging must retain applicable notices for bundled dependencies
and runtime packages. The media inspector is FFmpeg's separate `ffprobe`
executable; preserve the base distribution's package copyright and license
files. Do not describe all contents of a container as MIT solely because
beweft's application source is MIT.

No upstream code snippet should be copied into the project without checking its
license and retaining the required attribution. API requests implemented from
official documentation use beweft's own code.
