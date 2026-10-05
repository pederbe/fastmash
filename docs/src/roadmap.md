# Roadmap

Fastmash's direction, roughly in order. Plans change as we learn from users;
[issues](https://github.com/pederbe/fastmash/issues) and
[discussions](https://github.com/pederbe/fastmash/discussions) are the best
way to influence them.

## Next

- **Performance.** Close the remaining gaps where GNU datamash is faster
  (large sorts that spill to disk, the geometric mean of many distinct values,
  scientific-notation input sorted in language locales), guided by the
  published [benchmarks](benchmarks/index.md#where-gnu-datamash-is-still-faster).
- **Native sorting everywhere**, retiring the external `sort` route that some
  sorted jobs in the `C` locales still use where the system supports it.
- **Broader locale support.**
- **Packaging** in distribution repositories, conda-forge and Homebrew on Linux.

## Later

- **macOS on Apple Silicon.**
- **Further table workflows**, guided by ordinary data-processing needs and
  feedback on the existing CSV, health, selection and comparison features.

## Towards 1.0

Version 1.0 will mean: the command language is stable, the numerical profile
is stable, Fastmash is faster than GNU datamash across the benchmark families,
and the list of differences is short and deliberate.
