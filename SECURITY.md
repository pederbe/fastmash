# Security policy

Fastmash reads untrusted data, so security matters. Thank you for reporting
problems responsibly.

## Supported versions

Security fixes are made for the latest released version. Fastmash is before
1.0, so fixes ship in a new release rather than as backports.

| Version | Supported |
| --- | --- |
| Latest 0.x release | Yes |
| Older releases | No; please upgrade |

## Reporting a vulnerability

Please **do not** open a public issue for a security problem.

Report it privately through GitHub:
[Report a vulnerability](https://github.com/pederbe/fastmash/security/advisories/new)
(the **Security** tab → **Report a vulnerability**). If you can't use GitHub,
use the contact form at <https://pederbe.dev/#contact> and ask for a private
channel. Do not include exploit details in that first message.

Helpful details:

- The Fastmash version (`fastmash --version`) and how it was installed
- Your Linux distribution, kernel version, and whether you run under WSL
- A minimal command and input that reproduce the problem
- What you expected and what happened, including the exit status
- Your assessment of the impact

## What to expect

Fastmash is maintained by one person, so these are targets rather than
guarantees:

- An acknowledgement within **7 days**
- An initial assessment within **14 days**
- A fix or mitigation plan agreed with you before any public disclosure

Once a fix is released, we publish a GitHub security advisory and credit you,
unless you prefer to remain anonymous. We ask that you allow up to **90 days**
from your report before disclosing it publicly, and we will tell you if a fix
needs longer.

## Scope

In scope, for example:

- Memory-safety problems, crashes or hangs caused by crafted input or arguments
- Unbounded resource use that bypasses Fastmash's checked limits
- Problems with temporary files, including disclosure, symlink or cleanup issues
- Problems in how Fastmash starts or supervises the system `sort` command
- Incorrect results that a crafted input could exploit, where the impact goes
  beyond an ordinary bug

Out of scope:

- Running out of memory or disk on genuinely large input. Fastmash reports
  allocation failure where it can; the operating system may still stop it.
- MD5 and SHA-1 being weak hash functions. Fastmash provides them for
  compatibility with GNU datamash, not for security.
- The unseeded `rand` operation being predictable. It is not a cryptographic
  random source.
- Vulnerabilities in the system `sort`, the C library or the kernel. Please
  report those to their maintainers; tell us too if Fastmash should mitigate them.

## Security design

Fastmash is written in Rust, has no network access and does not execute user
input. It checks allocations and reports refusals instead of guessing, and it
runs the system `sort` only through a small supervisor with a fixed argument
list and a private temporary directory. See [ARCHITECTURE.md](ARCHITECTURE.md)
for details.
