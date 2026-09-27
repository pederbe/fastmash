//! Numeric input locale and output policy owned by one invocation.
use super::{Failure, conversion_failure, failure, named_fields, unsupported};
use fastmash_conversion::{
    format::{self, FormatId},
    presentation::{Invalid, Spec},
    profile::Profile,
};
use fastmash_numeric_contract::Value80;

pub(super) struct Presentation {
    pub profile: Profile,
    pub spec: Option<Spec>,
    pub utf8: bool,
}
impl Default for Presentation {
    fn default() -> Self {
        Self {
            profile: Profile::C,
            spec: None,
            utf8: false,
        }
    }
}
impl Presentation {
    pub fn render(&self, value: Value80) -> Result<Vec<u8>, Failure> {
        if let Some(spec) = &self.spec {
            spec.render(value, self.profile).map_err(conversion_failure)
        } else {
            format::format(value, self.profile, FormatId::Default14, 16384)
                .map(|v| v.bytes)
                .map_err(|e| conversion_failure(e.error))
        }
    }
    pub fn format(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        self.spec = Some(Spec::parse(bytes).map_err(|e| {
            if e == Invalid::TooLong {
                return failure(b"numeric format too large\n".to_vec());
            }
            if e == Invalid::Capacity {
                return unsupported("numeric presentation size exceeds addressable capacity");
            }
            let mut msg = b"format ".to_vec();
            named_fields::quote_with(bytes, &mut msg, self.utf8);
            match e {
                Invalid::NoDirective => msg.extend_from_slice(b" has no % directive"),
                Invalid::MissingType => msg.extend_from_slice(b" missing valid type after '%'"),
                Invalid::Type(b) => {
                    msg.extend_from_slice(b" has unknown/invalid type %");
                    msg.push(b);
                    msg.extend_from_slice(b" directive");
                }
                Invalid::Multiple => msg.extend_from_slice(b" has too many % directives"),
                _ => unreachable!(),
            }
            msg.push(b'\n');
            failure(msg)
        })?);
        Ok(())
    }
    pub fn round(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        if bytes.is_empty() {
            return Err(failure(b"missing rounding digits value\n".to_vec()));
        }
        let start = bytes
            .iter()
            .position(|b| !b.is_ascii_whitespace() && *b != 0x0b)
            .unwrap_or(bytes.len());
        let text = std::str::from_utf8(&bytes[start..]).unwrap_or("");
        if let Ok(n) = text.parse::<u32>()
            && (1..=50).contains(&n)
        {
            return self.format(format!("%.{n}f").as_bytes());
        }
        let mut msg = b"invalid rounding digits value ".to_vec();
        named_fields::quote_with(bytes, &mut msg, self.utf8);
        msg.push(b'\n');
        Err(failure(msg))
    }
}
