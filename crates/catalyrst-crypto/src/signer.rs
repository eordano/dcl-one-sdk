use std::fmt;

use catalyrst_types::EthAddress;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Signer(EthAddress);

impl Signer {
    pub(crate) fn from_verified_chain(address: &str) -> Self {
        Self(address.to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(any(test, feature = "test-signer"))]
    pub fn unchecked_for_test(address: &str) -> Self {
        Self(address.to_ascii_lowercase())
    }
}

impl AsRef<str> for Signer {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for Signer {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Signer {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for Signer {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl PartialEq<Signer> for str {
    fn eq(&self, other: &Signer) -> bool {
        self == other.0
    }
}

impl PartialEq<Signer> for String {
    fn eq(&self, other: &Signer) -> bool {
        self == &other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_signer_is_lowercased() {
        assert_eq!(Signer::from_verified_chain("0xABCDEF").as_str(), "0xabcdef");
    }

    #[test]
    fn compares_against_borrowed_and_owned_strings() {
        let signer = Signer::from_verified_chain("0xAb");
        assert_eq!(signer, "0xab");
        assert_eq!(signer, "0xab".to_string());
        assert_eq!(*"0xab", signer);
    }
}
