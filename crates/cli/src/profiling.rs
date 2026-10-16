//! Opt-in static diagnostics, independent of Command streams.
#![forbid(unsafe_code)]

#[cfg(feature = "hotpath-alloc")]
#[global_allocator]
static ALLOCATOR: hotpath::CountingAllocator = hotpath::CountingAllocator::new();

pub(super) fn start() -> Result<Option<hotpath::HotpathGuard>, &'static str> {
    let Some(path) = std::env::var_os("HOTPATH_OUTPUT_PATH") else {
        return Ok(None);
    };
    if !matches!(
        std::env::var("HOTPATH_METRICS_SERVER_OFF").as_deref(),
        Ok("1" | "true")
    ) {
        return Err("static profiling requires HOTPATH_METRICS_SERVER_OFF=1");
    }
    // Exclusive creation rejects standard streams, devices and existing data.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "profiling requires a new writable HOTPATH_OUTPUT_PATH file")?;
    Ok(Some(
        hotpath::HotpathGuardBuilder::new("fastmash::Command")
            .output_path(path)
            .format(hotpath::Format::Json)
            .functions_limit(0)
            .build(),
    ))
}
