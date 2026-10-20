#!/bin/bash
# The build prepares this Rust checker for the isolated package test directory.
set -euo pipefail
bin=$(dirname "$(command -v fastmash)")
exec ./fastmash-package-test "$bin"
