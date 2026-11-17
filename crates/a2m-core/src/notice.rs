//! Notices a door must show beside a surface: a closed list, each with
//! fixed words, which a pack's registration may give a surface and nothing
//! a caller's input can set (decision 0193).

use crate::vocabulary::vocabulary;

vocabulary! {
    /// Something about a surface that a person approving a write, or
    /// publishing a version, must be told. The list is closed, and each has
    /// fixed words ([`SurfaceNotice::words`]): a pack names one, and never
    /// writes the words itself.
    pub enum SurfaceNotice: "surface notice" {
        /// The target accepts that a credential crosses a link that is not
        /// encrypted to this surface (decision 0188).
        UnencryptedCredentials => "unencrypted_credentials",
    }
}

impl SurfaceNotice {
    /// The fixed words a door shows for the notice.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::UnencryptedCredentials => {
                "Credentials cross an unencrypted link to this surface: anyone on the network path can read them."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SurfaceNotice;

    #[test]
    fn a_notice_is_read_and_written_by_its_name_and_has_fixed_words() {
        for notice in SurfaceNotice::ALL {
            assert_eq!(notice.as_str().parse::<SurfaceNotice>().unwrap(), *notice);
            assert!(notice.words().ends_with('.'));
        }
        assert_eq!(
            serde_json::to_value(SurfaceNotice::UnencryptedCredentials).unwrap(),
            "unencrypted_credentials"
        );
        assert!("credentials".parse::<SurfaceNotice>().is_err());
    }
}
