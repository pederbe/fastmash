# 0007: Linux x86-64 first

Status: accepted.

Fastmash supports Linux x86-64 with glibc, and Windows through WSL2. Portable
arithmetic already makes results independent of the platform, but process
supervision, temporary files and signal handling are Linux-specific.

Support is kept to what can be tested and maintained with the hardware and
continuous integration available to the project: current desktop and laptop
Linux systems, one long-term-support Linux baseline for release binaries, and
hosted CI. macOS on Apple Silicon and native Windows are planned; each will be
added when it can be built and tested on the same terms.
