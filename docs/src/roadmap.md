# Roadmap

Fastmash's direction, roughly in order. Plans change as we learn from users;
[issues](https://github.com/pederbe/fastmash/issues) and
[discussions](https://github.com/pederbe/fastmash/discussions) are the best
way to influence them.

## Next

- **Performance.** Close the remaining gaps where GNU datamash is faster
  (mixed scientific statistics, alternative means, large sorts that spill to
  disk), guided by published benchmarks.
- **Native sorting everywhere**, retiring the external `sort` route and its
  Linux 5.11 requirement.
- **Broader locale support.**
- **Packaging** in distribution repositories, conda-forge and Homebrew on Linux.

## Later

- **macOS on Apple Silicon.**
- **Native Windows.**
- **Extensions beyond GNU datamash**, considered once compatibility and
  performance are solid. Candidates include quoted CSV input and custom
  output column names.

## Towards 1.0

Version 1.0 will mean: the command language is stable, the numerical profile
is stable, Fastmash is faster than GNU datamash across the benchmark families,
and the list of differences is short and deliberate.
