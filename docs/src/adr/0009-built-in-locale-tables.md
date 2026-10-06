# 0009: Built-in locale tables

Status: accepted.

**Decision:** Fastmash carries glibc's locale rules in its own tables and does
not read the host's locales at run time. The tables cover how numbers are read
and written in every UTF-8 locale glibc 2.43 defines, and how keys are sorted
in the language locales whose sorting has been verified against GNU sort.
They are identical on every machine and need no installed locale data, so a
job in a bare container gives the same result as on a workstation.

A locale name glibc does not know behaves as `C`, as it does in GNU datamash.
Fastmash refuses sorting in a locale whose collation is not verified (such as
`cs_CZ` or `nb_NO`) and numbers whose separators it cannot write (such as
`ps_AF`, or a non-UTF-8 character set with a non-ASCII thousands separator,
like `fr_FR` in Latin-1), rather than guess
([0004](0004-refuse-rather-than-guess.md)).

**Considered:** reading the host's locales, as GNU datamash does. That matches
GNU on each host, including its fallback to `C` where a locale is not
installed, but loses the reproducibility of
[0003](0003-portable-results.md), needs glibc's locale files at run time and a
second collation path, and greatly enlarges what must be tested.

**Consequences:**

- Fastmash differs from GNU datamash where a locale glibc knows is not
  installed on the host (GNU falls back to `C`, Fastmash uses its tables), and
  where the host's glibc locale data differs from 2.43.
- A job blocked by an unsupported locale is answered by adding that locale
  from glibc's data, not by reading host locales.
- The tables follow glibc 2.43. Regenerate them when a glibc release changes
  locale data that Fastmash uses, or when a user reports a difference:
  `scripts/generate_numeric_locales.py` writes the number table, checked with
  `scripts/compare_numeric_locales.py`; `scripts/verify_collation_locales.py`
  chooses and verifies the sorting locales; and
  `scripts/generate_collation_glibc.py` writes glibc's treatment of the
  characters it collates apart from letters. Their inputs and results are in
  `data/locales/`.
