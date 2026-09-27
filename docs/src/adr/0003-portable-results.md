# 0003: Portable results

Status: accepted.

**Decision:** one Fastmash version gives identical results for the same input
and explicit settings on every supported machine, provided that the
implementation meets the project's accuracy and performance goals. Command
compatibility with GNU datamash is kept, and specific numerical differences are
assessed and documented one by one.

The alternative was to match whatever the local GNU datamash build prints.
Different GNU builds can disagree in the last digit, so that target would be a
moving one, while portable results give one expected answer that tests, users
and CI can rely on. Matching the local GNU output byte for byte would ease some
migrations; that benefit was judged smaller.

**Current implementation:** software 80-bit arithmetic that preserves GNU
datamash's precision, range and order of evaluation, with `log` and `exp`
computed in software aiming for the correctly rounded result. The decision
fixes the property, not this particular engine: precision, algorithms and
dependencies may change, but any change to numerical output is versioned and
listed in the changelog.

**Consequences:**

- Fastmash can differ from a local GNU build in the last printed digit of some
  results, or the sign of a NaN. These differences are documented.
- Some extreme inputs that cannot be computed within fixed limits are refused
  (see [0004](0004-refuse-rather-than-guess.md)).
