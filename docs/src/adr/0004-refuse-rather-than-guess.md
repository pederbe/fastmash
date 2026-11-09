# 0004: Refuse rather than guess

Status: accepted.

When a feature, locale or checked resource condition is unsupported, Fastmash
stops with a clear diagnostic and exit status 77 (a *refusal*). Status 1 instead
identifies an error in the command or input. Refusals include non-integer
percentiles with undefined GNU datamash 1.9 results, unsupported locales and
checked allocation failures.

A separate status lets scripts and test harnesses distinguish "Fastmash
declined" from "the input is wrong".

**Alternative:** matching whatever GNU datamash prints, even when that output is
undefined. Rejected, because undefined output is not a compatibility target,
and a plausible wrong number is worse than a clear refusal.
