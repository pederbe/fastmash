# Portable musl copy routines

`memcpy.c`, `memmove.c` and `COPYRIGHT` are unmodified files from
[musl 1.2.6](https://musl.libc.org/releases/musl-1.2.6.tar.gz).

Linux musl builds select these portable implementations instead of musl's
x86-64 assembly routines. They reduced copying costs in the measured grouping
and sorting workloads. `build.rs` compiles them with the ordinary C compiler
and redirects the binary's copy and move calls through linker wrapping.
The baseline instruction set remains x86-64. Other targets keep their normal
library routines.

Keep the sources and upstream copyright together when updating them.
