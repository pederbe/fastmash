# 0001: Implement the GNU datamash command language, independently

Status: accepted.

Fastmash adopts GNU datamash 1.9's operations, options, selectors and data output
so that existing scripts can move over with few or no changes. Intentional
differences are [documented](../guide/differences.md).

Fastmash is an independent implementation under MIT OR Apache-2.0 and contains
no GNU code.

**Alternative:** a new command language. Rejected because
compatibility is the main reason an existing datamash user would try Fastmash.
