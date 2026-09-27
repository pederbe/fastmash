# 0004: Refuse rather than guess

Status: accepted.

When Fastmash cannot produce a result it can stand behind, it stops with a
clear diagnostic and exit status 77 (a *refusal*), distinct from status 1 for
errors in the command or input. Examples: a non-integer percentile, whose GNU
datamash 1.9 result is undefined; an unsupported locale, where numbers might be
read with the wrong decimal separator; a checked allocation that fails.

A separate status lets scripts and test harnesses distinguish "Fastmash
declined" from "the input is wrong".

**Considered:** matching whatever GNU datamash prints, even when that output is
undefined. Rejected, because undefined output is not a compatibility target,
and a plausible wrong number is worse than a clear refusal.
