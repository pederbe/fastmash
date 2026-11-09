# 0003: Portable results

Status: accepted.

One Fastmash version gives identical data results for the same input and
explicit settings on every supported machine. Arithmetic and locale rules are
built into the program. Differences from GNU datamash are
[documented individually](../guide/differences.md).

Matching each local GNU datamash build instead would ease some migrations, but
different builds can disagree in the last digit. Portable results provide one
expected answer across supported platforms.

Software 80-bit arithmetic preserves GNU datamash's precision, range and order
of evaluation. Software `log` and `exp` aim for correctly rounded results. The
engine may change, but any change to numerical output needs a new numerical
profile and a changelog entry.

**Consequences:**

- Fastmash can differ from a local GNU build in the last printed digit of some
  results, or the sign of a NaN. These differences are documented.
- Some extreme inputs that cannot be computed within fixed limits are refused
  (see [0004](0004-refuse-rather-than-guess.md)).
