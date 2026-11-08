//! One terminal palette and destination policy for human-readable output.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy)]
pub(super) enum Destination {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Detection {
    pub stdout_terminal: bool,
    pub stderr_terminal: bool,
    pub suppressed: bool,
}

impl Detection {
    #[forbid(unsafe_code)]
    pub(super) fn from_env() -> Self {
        Self {
            stdout_terminal: nix::unistd::isatty(rustix::stdio::stdout()).unwrap_or(false),
            stderr_terminal: nix::unistd::isatty(rustix::stdio::stderr()).unwrap_or(false),
            suppressed: environment_suppresses(),
        }
    }

    pub(super) fn style(self, mode: ColorMode, destination: Destination) -> Style {
        let terminal = match destination {
            Destination::Stdout => self.stdout_terminal,
            Destination::Stderr => self.stderr_terminal,
        };
        Style(match mode {
            ColorMode::Auto => terminal && !self.suppressed,
            ColorMode::Always => true,
            ColorMode::Never => false,
        })
    }
}

fn environment_suppresses() -> bool {
    // Read immediately and retain no borrowed environment pointers. The Command
    // never mutates its environment, including on its allocation-failure path.
    // SAFETY: both names are NUL-terminated; getenv returns null or a live
    // NUL-terminated value, and the process environment is not being mutated.
    unsafe {
        let term = libc::getenv(c"TERM".as_ptr());
        let no_color = libc::getenv(c"NO_COLOR".as_ptr());
        (!term.is_null() && std::ffi::CStr::from_ptr(term).to_bytes() == b"dumb")
            || (!no_color.is_null() && *no_color != 0)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Role {
    Heading,
    Syntax,
    Advisory,
    Failure,
    Value,
    Success,
    Muted,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Style(bool);

impl Style {
    pub(super) fn enabled(self) -> bool {
        self.0
    }

    pub(super) fn start(self, role: Role) -> &'static [u8] {
        if !self.0 {
            return b"";
        }
        match role {
            Role::Heading => b"\x1b[1;38;2;189;147;249m",
            Role::Syntax => b"\x1b[38;2;92;184;255m",
            Role::Value => b"\x1b[1;38;2;92;184;255m",
            Role::Advisory => b"\x1b[1;38;2;243;156;18m",
            Role::Failure => b"\x1b[1;38;2;255;92;87m",
            Role::Success => b"\x1b[1;38;2;90;247;142m",
            Role::Muted => b"\x1b[38;2;112;120;140m",
        }
    }

    pub(super) fn reset(self) -> &'static [u8] {
        if self.0 { b"\x1b[0m" } else { b"" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_detection_and_explicit_overrides_are_independent() {
        for stdout_terminal in [false, true] {
            for stderr_terminal in [false, true] {
                for suppressed in [false, true] {
                    let detection = Detection {
                        stdout_terminal,
                        stderr_terminal,
                        suppressed,
                    };
                    for (destination, terminal) in [
                        (Destination::Stdout, stdout_terminal),
                        (Destination::Stderr, stderr_terminal),
                    ] {
                        for (mode, enabled) in [
                            (ColorMode::Auto, terminal && !suppressed),
                            (ColorMode::Always, true),
                            (ColorMode::Never, false),
                        ] {
                            let style = detection.style(mode, destination);
                            for (role, sequence) in [
                                (Role::Heading, b"\x1b[1;38;2;189;147;249m".as_slice()),
                                (Role::Syntax, b"\x1b[38;2;92;184;255m"),
                                (Role::Advisory, b"\x1b[1;38;2;243;156;18m"),
                                (Role::Failure, b"\x1b[1;38;2;255;92;87m"),
                            ] {
                                assert_eq!(style.start(role), if enabled { sequence } else { b"" });
                            }
                            assert_eq!(
                                style.reset(),
                                if enabled { b"\x1b[0m".as_slice() } else { b"" }
                            );
                        }
                    }
                }
            }
        }
    }
}
