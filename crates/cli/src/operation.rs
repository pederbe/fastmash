//! The Operation catalog: every kind of Operation, how the command language
//! spells it, and what is fixed for each kind (the work it shares, the
//! numerics it needs, whether it runs per row or spans Groups).

use super::{checksum, line_numeric, numerics, ordered_statistics, path_fields};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Cut,
    Base64,
    Debase64,
    Checksum(checksum::Algorithm),
    Path(path_fields::Operation),
    Rounding(line_numeric::Rounding),
    Getnum(line_numeric::Extraction),
    Bin(fastmash_numeric_contract::Raw80),
    Strbin(std::num::NonZeroU64),
    Count,
    Countunique,
    Unique,
    Collapse,
    First,
    Last,
    Rand,
    Min,
    Max,
    Absmin,
    Absmax,
    Range,
    Sum,
    Mean,
    Wmean,
    Geomean,
    Harmmean,
    Ms,
    Rms,
    Median,
    Mode,
    Antimode,
    Q1,
    Q3,
    Iqr,
    Percentile(u8),
    Trimmean(ordered_statistics::Trim),
    Pvar,
    Svar,
    Pstdev,
    Sstdev,
    Madraw,
    Mad,
    Pskew,
    Sskew,
    Pkurt,
    Skurt,
    Jarque,
    Dpo,
    Pcov,
    Scov,
    Ppearson,
    Spearson,
    Dotprod,
}

/// Operations without a parameter, by name; `Kind::from_spelling` adds the rest.
pub(super) const PLAIN_OPERATIONS: &[(&[u8], Kind)] = &[
    (b"cut", Kind::Cut),
    (b"echo", Kind::Cut),
    (b"base64", Kind::Base64),
    (b"debase64", Kind::Debase64),
    (b"count", Kind::Count),
    (b"first", Kind::First),
    (b"countunique", Kind::Countunique),
    (b"unique", Kind::Unique),
    (b"uniq", Kind::Unique),
    (b"collapse", Kind::Collapse),
    (b"last", Kind::Last),
    (b"rand", Kind::Rand),
    (b"min", Kind::Min),
    (b"max", Kind::Max),
    (b"absmin", Kind::Absmin),
    (b"absmax", Kind::Absmax),
    (b"range", Kind::Range),
    (b"sum", Kind::Sum),
    (b"mean", Kind::Mean),
    (b"wmean", Kind::Wmean),
    (b"geomean", Kind::Geomean),
    (b"harmmean", Kind::Harmmean),
    (b"ms", Kind::Ms),
    (b"rms", Kind::Rms),
    (b"median", Kind::Median),
    (b"mode", Kind::Mode),
    (b"antimode", Kind::Antimode),
    (b"q1", Kind::Q1),
    (b"q3", Kind::Q3),
    (b"iqr", Kind::Iqr),
    (b"pvar", Kind::Pvar),
    (b"svar", Kind::Svar),
    (b"pstdev", Kind::Pstdev),
    (b"sstdev", Kind::Sstdev),
    (b"madraw", Kind::Madraw),
    (b"mad", Kind::Mad),
    (b"pskew", Kind::Pskew),
    (b"sskew", Kind::Sskew),
    (b"pkurt", Kind::Pkurt),
    (b"skurt", Kind::Skurt),
    (b"jarque", Kind::Jarque),
    (b"dpo", Kind::Dpo),
    (b"pcov", Kind::Pcov),
    (b"scov", Kind::Scov),
    (b"ppearson", Kind::Ppearson),
    (b"spearson", Kind::Spearson),
    (b"dotprod", Kind::Dotprod),
];

impl Kind {
    /// The canonical name, as headers and diagnostics print it.
    pub(super) fn name(self) -> &'static str {
        match self {
            Kind::Cut => "cut",
            Kind::Rounding(kind) => kind.name(),
            Kind::Getnum(_) => "getnum",
            Kind::Bin(_) => "bin",
            Kind::Strbin(_) => "strbin",
            Kind::Base64 => "base64",
            Kind::Debase64 => "debase64",
            Kind::Checksum(algorithm) => algorithm.name(),
            Kind::Path(kind) => kind.name(),
            Kind::Count => "count",
            Kind::Countunique => "countunique",
            Kind::Unique => "unique",
            Kind::Collapse => "collapse",
            Kind::First => "first",
            Kind::Last => "last",
            Kind::Rand => "rand",
            Kind::Min => "min",
            Kind::Max => "max",
            Kind::Absmin => "absmin",
            Kind::Absmax => "absmax",
            Kind::Range => "range",
            Kind::Sum => "sum",
            Kind::Mean => "mean",
            Kind::Wmean => "wmean",
            Kind::Geomean => "geomean",
            Kind::Harmmean => "harmmean",
            Kind::Ms => "ms",
            Kind::Rms => "rms",
            Kind::Median => "median",
            Kind::Mode => "mode",
            Kind::Antimode => "antimode",
            Kind::Q1 => "q1",
            Kind::Q3 => "q3",
            Kind::Iqr => "iqr",
            Kind::Percentile(_) => "perc",
            Kind::Trimmean(_) => "trimmean",
            Kind::Pvar => "pvar",
            Kind::Svar => "svar",
            Kind::Pstdev => "pstdev",
            Kind::Sstdev => "sstdev",
            Kind::Madraw => "madraw",
            Kind::Mad => "mad",
            Kind::Pskew => "pskew",
            Kind::Sskew => "sskew",
            Kind::Pkurt => "pkurt",
            Kind::Skurt => "skurt",
            Kind::Jarque => "jarque",
            Kind::Dpo => "dpo",
            Kind::Pcov => "pcov",
            Kind::Scov => "scov",
            Kind::Ppearson => "ppearson",
            Kind::Spearson => "spearson",
            Kind::Dotprod => "dotprod",
        }
    }

    /// The operation named `spelling` (ASCII case-insensitive), with its
    /// default parameter; `None` for names that are not operations.
    pub(super) fn from_spelling(spelling: &[u8]) -> Option<Self> {
        use super::line_numeric::Rounding;
        if let Some((_, kind)) = PLAIN_OPERATIONS
            .iter()
            .find(|(name, _)| spelling.eq_ignore_ascii_case(name))
        {
            return Some(*kind);
        }
        if let Some(kind) = [
            Rounding::Round,
            Rounding::Floor,
            Rounding::Ceil,
            Rounding::Trunc,
            Rounding::Frac,
        ]
        .into_iter()
        .find(|kind| spelling.eq_ignore_ascii_case(kind.name().as_bytes()))
        {
            return Some(Kind::Rounding(kind));
        }
        if let Some(kind) = super::path_fields::Operation::from_name(spelling) {
            return Some(Kind::Path(kind));
        }
        if let Some(algorithm) = super::checksum::Algorithm::from_name(spelling) {
            return Some(Kind::Checksum(algorithm));
        }
        let is = |name: &[u8]| spelling.eq_ignore_ascii_case(name);
        Some(if is(b"getnum") {
            Kind::Getnum(super::line_numeric::Extraction::Positive)
        } else if is(b"bin") {
            Kind::Bin(super::numerics::Numerics::value80(super::integer(100)).raw())
        } else if is(b"strbin") {
            Kind::Strbin(std::num::NonZeroU64::new(10).unwrap())
        } else if is(b"trimmean") {
            Kind::Trimmean(super::ordered_statistics::Trim::zero())
        } else if is(b"perc") {
            Kind::Percentile(95)
        } else {
            return None;
        })
    }

    /// Whether the results depend on the Groups before this one in sorted
    /// order, which hash grouping does not visit in that order: `rand` draws
    /// from one generator across all Groups.
    pub(super) fn crosses_groups(self) -> bool {
        match self {
            Kind::Rand => true,
            Kind::Cut
            | Kind::Base64
            | Kind::Debase64
            | Kind::Checksum(_)
            | Kind::Path(_)
            | Kind::Rounding(_)
            | Kind::Getnum(_)
            | Kind::Bin(_)
            | Kind::Strbin(_)
            | Kind::Count
            | Kind::Countunique
            | Kind::Unique
            | Kind::Collapse
            | Kind::First
            | Kind::Last
            | Kind::Min
            | Kind::Max
            | Kind::Absmin
            | Kind::Absmax
            | Kind::Range
            | Kind::Sum
            | Kind::Mean
            | Kind::Wmean
            | Kind::Geomean
            | Kind::Harmmean
            | Kind::Ms
            | Kind::Rms
            | Kind::Median
            | Kind::Mode
            | Kind::Antimode
            | Kind::Q1
            | Kind::Q3
            | Kind::Iqr
            | Kind::Percentile(_)
            | Kind::Trimmean(_)
            | Kind::Pvar
            | Kind::Svar
            | Kind::Pstdev
            | Kind::Sstdev
            | Kind::Madraw
            | Kind::Mad
            | Kind::Pskew
            | Kind::Sskew
            | Kind::Pkurt
            | Kind::Skurt
            | Kind::Jarque
            | Kind::Dpo
            | Kind::Pcov
            | Kind::Scov
            | Kind::Ppearson
            | Kind::Spearson
            | Kind::Dotprod => false,
        }
    }
    /// Whether the operation depends on the numeric locale: it reads numbers,
    /// or prints a number (counts only matter with an explicit format).
    pub(super) fn reads_or_prints_numbers(self, explicit_format: bool) -> bool {
        match self {
            Self::Cut
            | Self::Base64
            | Self::Debase64
            | Self::Checksum(_)
            | Self::Path(_)
            | Self::Unique
            | Self::Collapse
            | Self::First
            | Self::Last
            | Self::Rand => false,
            Self::Count | Self::Countunique => explicit_format,
            // Default output prints integers below 1e14 without a decimal point.
            Self::Strbin(buckets) => explicit_format || buckets.get() > 100_000_000_000_000,
            _ => true,
        }
    }
    pub(super) fn converts_numeric_field(self) -> bool {
        match self {
            Self::Min
            | Self::Max
            | Self::Absmin
            | Self::Absmax
            | Self::Range
            | Self::Sum
            | Self::Mean
            | Self::Geomean
            | Self::Harmmean
            | Self::Ms
            | Self::Rms
            | Self::Median
            | Self::Mode
            | Self::Antimode
            | Self::Q1
            | Self::Q3
            | Self::Iqr
            | Self::Percentile(_)
            | Self::Trimmean(_)
            | Self::Pvar
            | Self::Svar
            | Self::Pstdev
            | Self::Sstdev
            | Self::Madraw
            | Self::Mad
            | Self::Pskew
            | Self::Sskew
            | Self::Pkurt
            | Self::Skurt
            | Self::Jarque
            | Self::Dpo
            | Self::Rounding(_)
            | Self::Bin(_) => true,
            Self::Cut
            | Self::Base64
            | Self::Debase64
            | Self::Checksum(_)
            | Self::Path(_)
            | Self::Getnum(_)
            | Self::Strbin(_)
            | Self::Count
            | Self::Countunique
            | Self::Unique
            | Self::Collapse
            | Self::First
            | Self::Last
            | Self::Rand
            | Self::Pcov
            | Self::Scov
            | Self::Ppearson
            | Self::Spearson
            | Self::Wmean
            | Self::Dotprod => false,
        }
    }
    pub(super) fn is_line(self) -> bool {
        matches!(
            self,
            Self::Cut
                | Self::Base64
                | Self::Debase64
                | Self::Checksum(_)
                | Self::Path(_)
                | Self::Rounding(_)
                | Self::Getnum(_)
                | Self::Bin(_)
                | Self::Strbin(_)
        )
    }
    pub(super) fn uses_borrowed_number(self) -> bool {
        self.uses_shared_sorted_samples()
            || self.is_mad()
            || self.is_dispersion()
            || matches!(
                self,
                Self::Sum
                    | Self::Rounding(_)
                    | Self::Bin(_)
                    | Self::Mean
                    | Self::Wmean
                    | Self::Min
                    | Self::Max
                    | Self::Absmin
                    | Self::Absmax
                    | Self::Range
            )
    }
    pub(super) fn is_mad(self) -> bool {
        matches!(self, Self::Madraw | Self::Mad)
    }
    pub(super) fn is_dispersion(self) -> bool {
        matches!(self, Self::Pvar | Self::Svar | Self::Pstdev | Self::Sstdev)
    }
    pub(super) fn uses_shared_sorted_samples(self) -> bool {
        matches!(
            self,
            Self::Median
                | Self::Q1
                | Self::Q3
                | Self::Iqr
                | Self::Percentile(_)
                | Self::Trimmean(_)
                | Self::Mode
                | Self::Antimode
        )
    }
    pub(super) fn is_moment(self) -> bool {
        matches!(self, Self::Pskew | Self::Sskew | Self::Pkurt | Self::Skurt)
    }
    pub(super) fn is_normality(self) -> bool {
        matches!(self, Self::Jarque | Self::Dpo)
    }
    pub(super) fn is_paired(self) -> bool {
        self.keeps_pair_samples() || self == Self::Wmean
    }
    /// Pair-shaped input alone does not require retaining paired samples.
    pub(super) fn keeps_pair_samples(self) -> bool {
        matches!(
            self,
            Self::Pcov | Self::Scov | Self::Ppearson | Self::Spearson | Self::Dotprod
        )
    }
    /// The optional numerics this Operation's summary uses.
    pub(super) fn numeric_requirements(self) -> numerics::Requirements {
        let (square_root, mean_math) = match self {
            Self::Rms
            | Self::Pstdev
            | Self::Sstdev
            | Self::Ppearson
            | Self::Spearson
            | Self::Pskew
            | Self::Sskew => (true, false),
            Self::Jarque | Self::Dpo => (true, true),
            Self::Geomean => (false, true),
            Self::Cut
            | Self::Base64
            | Self::Debase64
            | Self::Checksum(_)
            | Self::Path(_)
            | Self::Rounding(_)
            | Self::Getnum(_)
            | Self::Bin(_)
            | Self::Strbin(_)
            | Self::Count
            | Self::Countunique
            | Self::Unique
            | Self::Collapse
            | Self::First
            | Self::Last
            | Self::Rand
            | Self::Min
            | Self::Max
            | Self::Absmin
            | Self::Absmax
            | Self::Range
            | Self::Sum
            | Self::Mean
            | Self::Wmean
            | Self::Harmmean
            | Self::Ms
            | Self::Median
            | Self::Mode
            | Self::Antimode
            | Self::Q1
            | Self::Q3
            | Self::Iqr
            | Self::Percentile(_)
            | Self::Trimmean(_)
            | Self::Pvar
            | Self::Svar
            | Self::Madraw
            | Self::Mad
            | Self::Pkurt
            | Self::Skurt
            | Self::Pcov
            | Self::Scov
            | Self::Dotprod => (false, false),
        };
        numerics::Requirements {
            square_root,
            mean_math,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every spelling of the command language, parameterised ones by their
    /// base names.
    fn spellings() -> impl Iterator<Item = &'static str> {
        PLAIN_OPERATIONS
            .iter()
            .map(|(name, _)| std::str::from_utf8(name).unwrap())
            .chain([
                "round", "floor", "ceil", "trunc", "frac", "getnum", "bin", "strbin", "trimmean",
                "perc", "md5", "sha1", "sha224", "sha256", "sha384", "sha512", "dirname",
                "basename", "extname", "barename",
            ])
    }

    #[test]
    fn every_kind_has_a_spelling_that_its_name_parses_back_to() {
        let mut kinds = std::collections::HashSet::new();
        for spelling in spellings() {
            let kind = Kind::from_spelling(spelling.as_bytes()).expect(spelling);
            kinds.insert(std::mem::discriminant(&kind));
            let canonical = kind.name();
            let again = Kind::from_spelling(canonical.as_bytes()).unwrap();
            assert_eq!(again.name(), canonical, "{spelling}");
            let upper = spelling.to_ascii_uppercase();
            assert_eq!(
                Kind::from_spelling(upper.as_bytes()),
                Some(kind),
                "{spelling}"
            );
        }
        // Each kind of the catalog is reached by some spelling.
        assert_eq!(kinds.len(), 53);
        for alias in ["echo", "uniq"] {
            assert_ne!(Kind::from_spelling(alias.as_bytes()).unwrap().name(), alias);
        }
        for other in ["transpose", "groupby", "", "sums", "mea"] {
            assert_eq!(Kind::from_spelling(other.as_bytes()), None, "{other}");
        }
    }

    #[test]
    fn only_rand_spans_groups_and_pairs_are_the_paired_statistics() {
        for spelling in spellings() {
            let kind = Kind::from_spelling(spelling.as_bytes()).unwrap();
            assert_eq!(kind.crosses_groups(), kind == Kind::Rand, "{spelling}");
            assert_eq!(
                kind.is_paired(),
                ["pcov", "scov", "ppearson", "spearson", "dotprod", "wmean"].contains(&kind.name()),
                "{spelling}"
            );
        }
    }
}
