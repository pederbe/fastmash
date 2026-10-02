//! Binding a Command's Selectors and Grouping keys to fields: by name from the
//! Input header, or by number without one. Every route binds through here,
//! once before its first record, and again after hash grouping restarts.

use super::{Failure, OperationSet, command_memory, grammar, named_fields, options};

/// A Command's Operations and Grouping keys, with the field names they select
/// until an Input header resolves them.
pub(super) struct Binding<'p> {
    /// The invoked program name, which GNU's `--full` warning prints once the
    /// Input header has been read.
    pub(super) program: &'p [u8],
    pub(super) operations: OperationSet,
    /// Grouping keys by field number: 0 for a named key that no Input header
    /// has resolved, which the system sort is then given (and reports).
    pub(super) keys: Vec<u64>,
    names: Vec<named_fields::Named>,
    key_names: Vec<named_fields::Named>,
}

impl<'p> Binding<'p> {
    pub(super) fn new(
        program: &'p [u8],
        requests: Vec<grammar::Request>,
        fields: Vec<grammar::Field>,
    ) -> Result<Self, Failure> {
        let (operations, names) = OperationSet::new(requests)?;
        let mut keys = Vec::new();
        let mut key_names = Vec::new();
        command_memory::reserve(&mut keys, fields.len())?;
        command_memory::reserve(&mut key_names, fields.len())?;
        for (index, field) in fields.into_iter().enumerate() {
            keys.push(match field {
                grammar::Field::Number(n) => n,
                grammar::Field::Name(name) => {
                    key_names.push(named_fields::Named {
                        operation: index,
                        target: named_fields::Target::Single,
                        name,
                    });
                    0
                }
            });
        }
        Ok(Self {
            program,
            operations,
            keys,
            names,
            key_names,
        })
    }

    /// Whether a Selector or Grouping key names a field, which needs an
    /// Input header.
    pub(super) fn names_fields(&self) -> bool {
        !self.names.is_empty() || !self.key_names.is_empty()
    }

    /// Binds from the Input header `record`: each name resolves to the first
    /// field it labels.
    pub(super) fn header(
        &mut self,
        record: &[u8],
        options: &options::Options,
    ) -> Result<(), Failure> {
        let (input, utf8) = (options.input, options.locale.utf8);
        let named = named_fields::resolve_with(&self.names, record, input, utf8)?;
        for (index, _, field) in named_fields::resolve_with(&self.key_names, record, input, utf8)? {
            self.keys[index] = field;
        }
        self.operations.bind(named)
    }

    /// Binds without an Input header: every Selector keeps its number.
    pub(super) fn numbered(&mut self) -> Result<(), Failure> {
        self.operations.bind([])
    }

    /// Whether a named Grouping key is still unresolved.
    pub(super) fn unresolved_keys(&self) -> bool {
        self.keys.contains(&0)
    }
}
