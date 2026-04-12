use std::{
    fmt,
    str::FromStr,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RuntimeFeature {
    TypeStmt,
    InlineTypeParams,
    GenericDefaults,
    TypingReadOnly,
    TypingTypeIs,
    TypingNoDefault,
    WarningsDeprecated,
    DeferredAnnotations,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EmitStyle {
    #[default]
    Compat,
    Native,
}

impl fmt::Display for EmitStyle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Compat => "compat",
            Self::Native => "native",
        })
    }
}

impl FromStr for EmitStyle {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.trim() {
            "compat" => Ok(Self::Compat),
            "native" => Ok(Self::Native),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct PythonTarget {
    pub major: u8,
    pub minor: u8,
}

impl PythonTarget {
    pub const PYTHON_3_10: Self = Self { major: 3, minor: 10 };
    pub const PYTHON_3_11: Self = Self { major: 3, minor: 11 };
    pub const PYTHON_3_12: Self = Self { major: 3, minor: 12 };
    pub const PYTHON_3_13: Self = Self { major: 3, minor: 13 };
    pub const PYTHON_3_14: Self = Self { major: 3, minor: 14 };

    #[must_use]
    pub const fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (major, minor) = text.trim().split_once('.')?;
        Some(Self::new(major.parse().ok()?, minor.parse().ok()?))
    }

    #[must_use]
    pub fn default_emit_style(self) -> EmitStyle {
        if self >= Self::PYTHON_3_13 { EmitStyle::Native } else { EmitStyle::Compat }
    }

    #[must_use]
    pub fn supports(self, feature: RuntimeFeature) -> bool {
        self >= Self::min_runtime_for(feature)
    }

    #[must_use]
    pub const fn min_runtime_for(feature: RuntimeFeature) -> Self {
        match feature {
            RuntimeFeature::TypeStmt | RuntimeFeature::InlineTypeParams => Self::PYTHON_3_12,
            RuntimeFeature::GenericDefaults
            | RuntimeFeature::TypingReadOnly
            | RuntimeFeature::TypingTypeIs
            | RuntimeFeature::TypingNoDefault
            | RuntimeFeature::WarningsDeprecated => Self::PYTHON_3_13,
            RuntimeFeature::DeferredAnnotations => Self::PYTHON_3_14,
        }
    }

    #[must_use]
    pub fn supports_type_stmt(self) -> bool {
        self.supports(RuntimeFeature::TypeStmt)
    }

    #[must_use]
    pub fn supports_inline_type_params(self) -> bool {
        self.supports(RuntimeFeature::InlineTypeParams)
    }

    #[must_use]
    pub fn supports_generic_defaults(self) -> bool {
        self.supports(RuntimeFeature::GenericDefaults)
    }

    #[must_use]
    pub fn stdlib_owner(self, symbol: &str) -> Option<&'static str> {
        match symbol {
            "Self" => Some(if self >= Self::PYTHON_3_11 { "typing" } else { "typing_extensions" }),
            "Required" | "NotRequired" | "dataclass_transform" => {
                Some(if self >= Self::PYTHON_3_11 { "typing" } else { "typing_extensions" })
            }
            "override" => {
                Some(if self >= Self::PYTHON_3_12 { "typing" } else { "typing_extensions" })
            }
            "TypeVarTuple" | "Unpack" => {
                Some(if self >= Self::PYTHON_3_11 { "typing" } else { "typing_extensions" })
            }
            "ReadOnly" => {
                Some(if self.supports(RuntimeFeature::TypingReadOnly) {
                    "typing"
                } else {
                    "typing_extensions"
                })
            }
            "TypeIs" => Some(if self.supports(RuntimeFeature::TypingTypeIs) {
                "typing"
            } else {
                "typing_extensions"
            }),
            "NoDefault" => Some(if self.supports(RuntimeFeature::TypingNoDefault) {
                "typing"
            } else {
                "typing_extensions"
            }),
            "deprecated" => Some(if self.supports(RuntimeFeature::WarningsDeprecated) {
                "warnings"
            } else {
                "typing_extensions"
            }),
            _ => None,
        }
    }

    #[must_use]
    pub fn normalized_text(self) -> String {
        self.to_string()
    }
}

impl Default for PythonTarget {
    fn default() -> Self {
        Self::PYTHON_3_10
    }
}

impl fmt::Display for PythonTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for PythonTarget {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::{EmitStyle, PythonTarget, RuntimeFeature};

    #[test]
    fn target_defaults_to_native_only_for_313_plus() {
        assert_eq!(PythonTarget::PYTHON_3_12.default_emit_style(), EmitStyle::Compat);
        assert_eq!(PythonTarget::PYTHON_3_13.default_emit_style(), EmitStyle::Native);
    }

    #[test]
    fn stdlib_owner_tracks_runtime_capabilities() {
        assert_eq!(PythonTarget::PYTHON_3_10.stdlib_owner("Self"), Some("typing_extensions"));
        assert_eq!(PythonTarget::PYTHON_3_11.stdlib_owner("Self"), Some("typing"));
        assert_eq!(PythonTarget::PYTHON_3_12.stdlib_owner("deprecated"), Some("typing_extensions"));
        assert_eq!(PythonTarget::PYTHON_3_13.stdlib_owner("deprecated"), Some("warnings"));
        assert_eq!(PythonTarget::PYTHON_3_13.stdlib_owner("ReadOnly"), Some("typing"));
    }

    #[test]
    fn feature_minimums_are_stable() {
        assert!(PythonTarget::PYTHON_3_12.supports(RuntimeFeature::TypeStmt));
        assert!(!PythonTarget::PYTHON_3_12.supports(RuntimeFeature::GenericDefaults));
        assert!(PythonTarget::PYTHON_3_13.supports(RuntimeFeature::GenericDefaults));
        assert!(PythonTarget::PYTHON_3_14.supports(RuntimeFeature::DeferredAnnotations));
    }
}
