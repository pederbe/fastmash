//! Controlled Command transports shared by report format checks.
//!
//! For mixed-length byte fixtures, use explicit slice types. Examples are in
//! the Rust test fixtures section of the workspace guide (`crates/README.md`).
use super::*;

pub(super) struct Input<'a> {
    pub bytes: &'a [u8],
    pub segment: usize,
    pub error: Option<i32>,
}

impl io::Read for Input<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = buffer.len().min(available.len());
        buffer[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for Input<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.bytes.is_empty()
            && let Some(code) = self.error
        {
            return Err(io::Error::from_raw_os_error(code));
        }
        Ok(&self.bytes[..self.bytes.len().min(self.segment)])
    }
    fn consume(&mut self, count: usize) {
        self.bytes = &self.bytes[count..];
    }
}
impl replay::Rewind for Input<'_> {}

pub(super) struct Output {
    pub bytes: Vec<u8>,
    pub error: Option<i32>,
    pub closed: bool,
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(code) = self.error {
            return Err(io::Error::from_raw_os_error(code));
        }
        // Short writes exercise complete byte preservation without coupling to
        // the header renderer's internal calls.
        let count = bytes.len().min(2);
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl command_output::Transport for Output {
    fn buffering(&self) -> (usize, bool) {
        (1, false)
    }
    fn close(&mut self) -> io::Result<()> {
        self.closed = true;
        Ok(())
    }
}

pub(super) fn command(
    input: &mut Input<'_>,
    output: &mut impl command_output::Transport,
    args: &[&str],
) -> (i32, Vec<Vec<u8>>) {
    let environment = Environment {
        locale: Default::default(),
        posixly_correct: false,
        grouping: None,
        sort_memory: None,
        pipe_grouping: None,
        terminal: Default::default(),
    };
    command_in(input, output, args, environment)
}

/// Counts the existing checked reservations before dispatch so a Command test
/// can refuse its first Binding reservation without fixing parser counts.
pub(super) fn scanning_reservations(args: &[&str]) -> usize {
    let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
    command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(usize::MAX)));
    let options::Action::Calculate(options) =
        options::parse(&args, b"fastmash", false).ok().unwrap()
    else {
        panic!("calculation");
    };
    grammar::command_for_format(
        &options.operands,
        options.group.as_ref(),
        options.vnlog,
        options.presentation.profile,
        options.locale.utf8,
    )
    .ok()
    .unwrap();
    let remaining = command_memory::FAIL_RESERVATION.with(|slot| slot.replace(None).unwrap());
    usize::MAX - remaining
}

pub(super) fn command_in(
    input: &mut Input<'_>,
    output: &mut impl command_output::Transport,
    args: &[&str],
    environment: Environment,
) -> (i32, Vec<Vec<u8>>) {
    let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
    let mut diagnostics = Vec::new();
    let mut report = |failure: &Failure| {
        diagnostics.push(failure.message.clone());
        true
    };
    let status = run_in(input, output, &args, b"fastmash", &mut report, environment)
        .unwrap_or_else(|failure| {
            report(&failure);
            failure.status
        });
    (status, diagnostics)
}

pub(super) struct Sources<'a> {
    pub inputs: [Input<'a>; 2],
    pub closes: [Option<i32>; 2],
    pub opened: usize,
    pub completed: usize,
}
impl command_sources::Resolver for Sources<'_> {
    fn with_source<T>(
        &mut self,
        _: &mut dyn BufRead,
        _: &std::ffi::OsStr,
        scan: impl FnOnce(&mut dyn BufRead) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        let at = self.opened;
        self.opened += 1;
        let result = scan(&mut self.inputs[at]);
        self.completed += 1;
        let completion = self.closes[at].map_or(Ok(()), |code| {
            Err(command_sources::close_failure(
                &io::Error::from_raw_os_error(code),
            ))
        });
        command_sources::complete(result, completion)
    }
}

pub(super) fn comparison_command(
    args: &[&str],
    before: &[u8],
    after: &[u8],
    output: &mut impl command_output::Transport,
) -> (i32, Vec<Vec<u8>>) {
    let mut sources = Sources {
        inputs: [
            Input {
                bytes: before,
                segment: 3,
                error: None,
            },
            Input {
                bytes: after,
                segment: 2,
                error: None,
            },
        ],
        closes: [None, None],
        opened: 0,
        completed: 0,
    };
    command_sources(&mut sources, output, args)
}

pub(super) fn command_sources(
    sources: &mut impl command_sources::Resolver,
    output: &mut impl command_output::Transport,
    args: &[&str],
) -> (i32, Vec<Vec<u8>>) {
    let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
    let mut input = Input {
        bytes: b"",
        segment: 1,
        error: None,
    };
    let mut diagnostics = Vec::new();
    let mut report = |failure: &Failure| {
        diagnostics.push(failure.message.clone());
        true
    };
    let environment = Environment {
        locale: Default::default(),
        posixly_correct: false,
        grouping: None,
        sort_memory: None,
        pipe_grouping: None,
        terminal: Default::default(),
    };
    let status = run_with_sources(
        (&mut input, sources),
        output,
        &args,
        b"fastmash",
        &mut report,
        &mut random::OsSeedSource,
        environment,
    )
    .unwrap_or_else(|failure| {
        report(&failure);
        failure.status
    });
    (status, diagnostics)
}
