# Decision records

These records explain consequential design choices and the alternatives behind
them. Start with the [architecture](../../../ARCHITECTURE.md) for the
current design, then read the relevant record before changing a decision.

## Current decisions

Some records have been partly superseded. Their status identifies which choices
remain in force and links to later decisions. A change to a released decision
gets a new record. Editorial corrections keep the existing record and URL.

| Record | Decision |
| --- | --- |
| [0001](0001-gnu-command-language.md) | Implement the GNU datamash command language, independently |
| [0002](0002-rust-without-c-numerics.md) | Rust for the program and numerical libraries |
| [0003](0003-portable-results.md) | Portable results through software arithmetic |
| [0004](0004-refuse-rather-than-guess.md) | Refuse rather than guess |
| [0005](0005-growable-checked-storage.md) | Growable, checked storage instead of fixed limits |
| [0008](0008-hash-grouping-with-restart.md) | Group sorted input by hash, and sort again when unsure |
| [0009](0009-built-in-locale-tables.md) | Built-in locale tables, not the host's locales |
| [0010](0010-native-apple-silicon.md) | Native macOS support, with the development-only delivery boundary superseded |
| [0011](0011-native-release-delivery.md) | Deliver the same reproducible native archive that passed installation checks, with the original release procedure partly superseded |
| [0012](0012-release-delivery-and-performance.md) | Separate release delivery from performance acceptance |
| [0013](0013-reuse-tested-release-artifacts.md) | Reuse tested release artifacts |
| [0014](0014-native-sorting-everywhere.md) | Native sorting in one executable |
| [0015](0015-stable-release-toolchain.md) | Build releases with current stable Rust |
| [0016](0016-release-plz.md) | Version releases with release-plz while retaining qualified publication |
| [0017](0017-audited-startup.md) | Keep one audited startup module for inherited streams and Linux entry |
| [0018](https://github.com/pederbe/fastmash/blob/main/docs/src/adr/0018-static-linux-delivery.md) | Deliver static Linux binaries with musl |
| [0019](0019-one-product-crate.md) | Publish one product binary crate and retain separate maintenance tools |

## Superseded decisions

These describe earlier designs, not current requirements.

| Record | Replaced by |
| --- | --- |
| [0006: Supervised external sort](0006-supervised-external-sort.md) | [0014: Native sorting in one executable](0014-native-sorting-everywhere.md) |
| [0007: Linux first](0007-linux-first.md) | [0011: Native release delivery](0011-native-release-delivery.md), which published macOS support |
