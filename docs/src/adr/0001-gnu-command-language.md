# 0001: Implement the GNU datamash command language, independently

Status: accepted.

GNU datamash's command language is widely used in scripts and documentation.
Fastmash adopts it (operations, options, selectors, output layout and exit
statuses 0 and 1) so that existing workflows move over with few or no changes,
and aims to match GNU datamash 1.9's observable results.

Fastmash is an independent implementation. It contains no GNU code, so it can
be licensed under MIT OR Apache-2.0. Behavior is matched by studying GNU
datamash's documentation, source code and observed output, and every
intentional difference is published.

**Considered:** a new, cleaner command language. Rejected, because
compatibility is the main reason an existing datamash user would try Fastmash.
