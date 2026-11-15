//! Windows application buffering has a fixed byte capacity for redirected I/O.
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct Destination {
    pub isatty: bool,
    pub block_size: u64,
}
