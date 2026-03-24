use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::errors::IdValidationError;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd, Hash)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdValidationError> {
                Self::try_from(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                if value.trim().is_empty() {
                    return Err(IdValidationError::Empty {
                        kind: stringify!($name),
                    });
                }
                Ok(Self(value))
            }
        }
    };
}

string_id!(ClientRequestId);
string_id!(DeliveryLeaseId);
string_id!(PayloadHash);
string_id!(PresentationId);
string_id!(RequestId);
string_id!(WalletInstanceId);
