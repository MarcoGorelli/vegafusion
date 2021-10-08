//! A variable is named value in a Vega specification. These values can be signals, scales, or
//! datasets.

use serde::{Deserialize, Serialize};

/// The namespace for a variable. It's valid for the same name to be used for a signal, scale,
/// and dataset at the same time
#[derive(Debug, Clone, PartialOrd, PartialEq, Eq, Serialize, Deserialize, Hash, Ord)]
pub enum VariableNamespace {
    Signal,
    Scale,
    Data,
}

/// An (unscoped) variable
#[derive(Debug, Clone, PartialOrd, PartialEq, Eq, Serialize, Deserialize, Hash, Ord)]
pub struct Variable {
    pub namespace: VariableNamespace,
    pub name: String,
}

impl Variable {
    pub fn new(ns: VariableNamespace, name: &str) -> Self {
        if name.contains(':') {
            panic!("Variable names may not contain colons")
        }
        Self {
            namespace: ns,
            name: String::from(name),
        }
    }

    pub fn new_signal(id: &str) -> Self {
        Self::new(VariableNamespace::Signal, id)
    }

    pub fn new_scale(id: &str) -> Self {
        Self::new(VariableNamespace::Scale, id)
    }

    pub fn new_data(id: &str) -> Self {
        Self::new(VariableNamespace::Data, id)
    }
}

/// A variable with scope.  In a Vega spec, variables may be defined at the top-level, or they
/// may be defined inside nested group marks. The scope is a `Vec<usize>` and it encodes the level
/// at which the variable is defined.
///   - An empty scope corresponds to a variable defined at the top level of the specification.
///   - Each element of the scope vector represents a level of nesting, where the integer is the
///     index into the collection of group marks.
///
/// A scope of `vec![0, 1]` means that the variable is defined in a group mark that is nested two
/// levels deep. At the top level, the first group mark is chosen. At the second level,
/// the second group mark is chosen.
#[derive(Clone, Debug, PartialOrd, PartialEq, Eq, Serialize, Deserialize, Hash, Ord)]
pub struct ScopedVariable {
    pub variable: Variable,
    pub scope: Vec<usize>,
}

impl ScopedVariable {
    pub fn new(ns: VariableNamespace, name: &str, scope: Vec<usize>) -> Self {
        Self {
            variable: Variable::new(ns, name),
            scope,
        }
    }

    pub fn new_signal(id: &str, scope: Vec<usize>) -> Self {
        Self::new(VariableNamespace::Signal, id, scope)
    }

    pub fn new_scale(id: &str, scope: Vec<usize>) -> Self {
        Self::new(VariableNamespace::Scale, id, scope)
    }

    pub fn new_data(id: &str, scope: Vec<usize>) -> Self {
        Self::new(VariableNamespace::Data, id, scope)
    }
}
