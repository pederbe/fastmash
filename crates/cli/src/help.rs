//! Help's existing text with styling confined to headings and syntax.
use super::terminal_style::{Role, Style};
use std::io::{self, Write};

const TEXT: &[u8] = include_bytes!("help.txt");
const HEADINGS: &[&[u8]] = &[
    b"Usage:",
    b"Top-N selection:",
    b"Dataset comparison:",
    b"Operations:",
    b"Options:",
    b"Examples:",
    b"Runtime and numerical limits:",
    b"Per-row numeric operations:",
    b"Field modes:",
    b"Binning:",
    b"Table modes:",
    b"Table health:",
];

#[derive(Clone, Copy, Default)]
enum Section {
    #[default]
    Prose,
    Usage,
    Operations,
    Examples,
}

const USAGE_NAMES: &[&[u8]] = &[
    b"fastmash",
    b"top",
    b"bottom",
    b"compare",
    b"rank",
    b"limit",
    b"health",
    b"examples",
    b"type",
    b"required",
    b"nonmissing",
    b"width",
    b"validate",
    b"tsv",
];

struct Declaration {
    lead: &'static [u8],
    syntax: &'static [u8],
    names: &'static [&'static [u8]],
}

const fn declaration(
    lead: &'static [u8],
    syntax: &'static [u8],
    names: &'static [&'static [u8]],
) -> Declaration {
    Declaration {
        lead,
        syntax,
        names,
    }
}

// Only primary lookup declarations are annotated. Their exact fragments keep
// parameter values, selector placeholders and nearby prose in default styling.
const DECLARATIONS: &[Declaration] = &[
    declaration(b" ", b"top:N FIELD", &[b"top"]),
    declaration(b"", b"bottom:N FIELD", &[b"bottom"]),
    declaration(b" ", b"compare BEFORE AFTER", &[b"compare"]),
    declaration(
        b"Before operations, ",
        b"rank RESULT [absolute|percent]",
        &[b"rank"],
    ),
    declaration(b"Optional ", b"limit N", &[b"limit"]),
    declaration(
        b"Grouping also accepts ",
        b"groupby, grouping or gb",
        &[b"groupby", b"grouping", b"gb"],
    ),
    declaration(
        b"",
        b"Crosstab/ct KEY1,KEY2 [OP FIELD]",
        &[b"Crosstab", b"ct"],
    ),
    declaration(b"", b"cut/echo FIELD", &[b"cut", b"echo"]),
    declaration(b" ", b"reverse reverses", &[b"reverse"]),
    declaration(
        b"changing widths); ",
        b"nop/noop consumes",
        &[b"nop", b"noop"],
    ),
    declaration(b" ", b"getnum[:TYPE] FIELD", &[b"getnum"]),
    declaration(
        b"",
        b"round, floor, ceil, trunc and frac FIELD",
        &[b"round", b"floor", b"ceil", b"trunc", b"frac"],
    ),
    declaration(b" ", b"bin[:WIDTH] FIELD", &[b"bin"]),
    declaration(b"width 100). ", b"strbin[:COUNT] FIELD", &[b"strbin"]),
    declaration(b" ", b"check [N lines] [N fields]", &[b"check"]),
    declaration(b"", b"transpose exchanges", &[b"transpose"]),
    declaration(b"", b"rmdup/dedup FIELD", &[b"rmdup", b"dedup"]),
    declaration(b" ", b"health inspects", &[b"health"]),
    declaration(b"", b"type FIELD integer|number|text", &[b"type"]),
    declaration(
        b"",
        b"required FIELD rejects absent and empty values; nonmissing FIELD",
        &[b"required", b"nonmissing"],
    ),
    declaration(
        b"rule. Literal NA can pass required/text checks. ",
        b"width N",
        &[b"width"],
    ),
    declaration(b"findings. ", b"validate requires", &[b"validate"]),
    declaration(b"", b"examples N", &[b"examples"]),
    declaration(b"", b"tsv selects", &[b"tsv"]),
    declaration(b"", b"base64 FIELD", &[b"base64"]),
    declaration(b"", b"debase64 FIELD", &[b"debase64"]),
    declaration(
        b"",
        b"md5, sha1, sha224, sha256, sha384 and sha512 FIELD",
        &[b"md5", b"sha1", b"sha224", b"sha256", b"sha384", b"sha512"],
    ),
    declaration(
        b"",
        b"dirname, basename, extname and barename FIELD",
        &[b"dirname", b"basename", b"extname", b"barename"],
    ),
];

fn span(writer: &mut impl Write, style: Style, role: Role, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(style.start(role))?;
    writer.write_all(bytes)?;
    writer.write_all(style.reset())
}

/// Highlight literal option spellings, leaving their values and surrounding
/// prose in the default foreground. Examples never pass through this renderer.
fn options(writer: &mut impl Write, style: Style, bytes: &[u8]) -> io::Result<()> {
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        let boundary = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        if bytes[at] == b'-'
            && boundary
            && bytes
                .get(at + 1)
                .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'-')
        {
            writer.write_all(&bytes[start..at])?;
            let mut end = at + 2;
            while bytes
                .get(end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            {
                end += 1;
            }
            let token = &bytes[at..end];
            if super::options::documented_option(token) {
                span(writer, style, Role::Syntax, token)?;
            } else {
                writer.write_all(token)?;
            }
            at = end;
            start = end;
        } else {
            at += 1;
        }
    }
    writer.write_all(&bytes[start..])
}

fn names(
    writer: &mut impl Write,
    style: Style,
    bytes: &[u8],
    is_name: impl Fn(&[u8]) -> bool,
) -> io::Result<()> {
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_alphabetic() {
            writer.write_all(&bytes[start..at])?;
            let mut end = at + 1;
            while bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
                end += 1;
            }
            let word = &bytes[at..end];
            if is_name(word) {
                span(writer, style, Role::Syntax, word)?;
            } else {
                writer.write_all(word)?;
            }
            at = end;
            start = end;
        } else {
            at += 1;
        }
    }
    writer.write_all(&bytes[start..])
}

fn prose(writer: &mut impl Write, style: Style, bytes: &[u8]) -> io::Result<()> {
    for declaration in DECLARATIONS {
        if let Some(rest) = bytes.strip_prefix(declaration.lead)
            && let Some(tail) = rest.strip_prefix(declaration.syntax)
        {
            writer.write_all(declaration.lead)?;
            names(writer, style, declaration.syntax, |word| {
                declaration.names.contains(&word)
            })?;
            return options(writer, style, tail);
        }
    }
    options(writer, style, bytes)
}

pub(super) fn write(writer: &mut impl Write, style: Style) -> io::Result<()> {
    if style.start(Role::Heading).is_empty() {
        return writer.write_all(TEXT).and_then(|()| writer.flush());
    }
    let mut section = Section::Prose;
    for line in TEXT.split_inclusive(|byte| *byte == b'\n') {
        if line == b"\n" {
            section = Section::Prose;
        }
        if matches!(section, Section::Examples) {
            writer.write_all(line)?;
            continue;
        }
        let heading = HEADINGS
            .iter()
            .copied()
            .find(|heading| line.starts_with(heading));
        let rest = if let Some(heading) = heading {
            section = match heading {
                b"Usage:" => Section::Usage,
                b"Operations:" => Section::Operations,
                b"Examples:" => Section::Examples,
                _ => Section::Prose,
            };
            span(writer, style, Role::Heading, heading)?;
            &line[heading.len()..]
        } else {
            line
        };
        if matches!(section, Section::Usage) {
            names(writer, style, rest, |word| USAGE_NAMES.contains(&word))?;
        } else if matches!(section, Section::Operations)
            && line.starts_with(b"  ")
            && line.get(2).is_some_and(u8::is_ascii_lowercase)
        {
            let end = rest[2..]
                .windows(2)
                .position(|pair| pair == b"  ")
                .map_or(rest.len(), |at| at + 2);
            names(writer, style, &rest[..end], |word| {
                word[0].is_ascii_lowercase()
            })?;
            writer.write_all(&rest[end..])?;
        } else {
            prose(writer, style, rest)?;
        }
    }
    // Every span resets before subsequent bytes; finish in the default style.
    writer
        .write_all(style.reset())
        .and_then(|()| writer.flush())
}
