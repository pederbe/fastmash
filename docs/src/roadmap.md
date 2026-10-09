# Roadmap

Fastmash's direction, roughly in order. Plans change as we learn from users;
[issues](https://github.com/pederbe/fastmash/issues) and
[discussions](https://github.com/pederbe/fastmash/discussions) are the best
way to influence them.

## 0.2.0: macOS on Apple Silicon

Native Apple Silicon macOS support is the main feature planned for 0.2.0.
The candidate has native builds, complete command and numerical checks,
and archive installation on macOS 15 and 26. Release delivery builds twice,
checks one frozen archive on both versions, and repeats installation against
the draft's uploaded bytes. Linux remains supported. Published downloads
remain at 0.1.0 until 0.2.0 qualification and publication are complete.

## Other planned work

- **Performance.** Close the remaining gaps where GNU datamash is faster
  (large sorts that spill to disk, the geometric mean of many distinct values,
  scientific-notation input sorted in language locales), guided by the
  published [benchmarks](benchmarks/index.md#where-gnu-datamash-is-still-faster).
- **Native sorting everywhere**, retiring the external `sort` route that some
  sorted jobs in the `C` locales still use where the system supports it.
- **Broader locale support.**
- **Packaging** in distribution repositories, conda-forge and Homebrew on Linux.

## Later

- **Further table workflows**, guided by ordinary data-processing needs and
  feedback on the existing CSV, health, selection and comparison features.

## Towards 1.0

Version 1.0 will mean: the command language is stable, the numerical profile
is stable, Fastmash is faster than GNU datamash across the benchmark families,
and the list of differences is short and deliberate.
