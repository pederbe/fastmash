# Roadmap

Fastmash's direction, roughly in order. Plans change as we learn from users;
[issues](https://github.com/pederbe/fastmash/issues) and
[discussions](https://github.com/pederbe/fastmash/discussions) are the best
way to influence them.

## Available in 0.2.0: macOS on Apple Silicon

Fastmash 0.2.0 adds native Apple Silicon macOS 15 and later alongside Linux
x86-64 and WSL2. Its published archive is checked on macOS 15 and 26,
including command behavior, numerical results and archive installation.

## In development: native sorting everywhere

Every sorted calculation and sorted duplicate removal now uses built-in stable
sorting with disk spill. Fastmash builds as one executable on Linux and macOS.
The next release will no longer require the system `sort` or a sort supervisor.

## Other planned work

- **Performance.** Close the remaining gaps where GNU datamash is faster
  (large sorts that spill to disk, the geometric mean of many distinct values,
  scientific-notation input sorted in language locales), guided by the
  published [benchmarks](benchmarks/index.md#where-gnu-datamash-is-still-faster).
- **Broader locale support.**
- **Packaging** in distribution repositories and conda-forge.

## Later

- **Further table workflows**, guided by ordinary data-processing needs and
  feedback on the existing CSV, health, selection and comparison features.

## Towards 1.0

Version 1.0 will mean: the command language is stable, the numerical profile
is stable, Fastmash is faster than GNU datamash across the benchmark families,
and the list of differences is short and deliberate.
