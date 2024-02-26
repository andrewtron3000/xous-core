// A rkyv serialisable intermediatory for a TrustAnchor
use rkyv::{Archive, Deserialize, Serialize};
use rustls::pki_types::{Der, TrustAnchor};
use std::cmp::min;
use std::fmt;
use x509_parser::prelude::{FromDer, X509Certificate};

pub const MAX_OTA_BYTES: usize = 1028;

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct OwnedTrustAnchor {
    pub subject: Vec<u8>,
    pub spki: Vec<u8>,
    pub name_constraints: Option<Vec<u8>>,
}

impl OwnedTrustAnchor {
    pub fn from_subject_spki_name_constraints(
        subject: impl Into<Vec<u8>>,
        spki: impl Into<Vec<u8>>,
        name_constraints: Option<impl Into<Vec<u8>>>,
    ) -> Self {
        Self {
            subject: subject.into(),
            spki: spki.into(),
            name_constraints: name_constraints.map(|x| x.into()),
        }
    }

    pub fn pddb_key(&self) -> String {
        let subject = match std::str::from_utf8(&self.subject) {
            Ok(subject) => subject,
            Err(e) => {
                log::warn!("failed to extract ota.subject: {e}");
                ""
            }
        };
        let begin = match subject.find("CN=") {
            Some(begin) => Some(begin),
            None => subject.find("OU="),
        };

        let mut pddb_key = match begin {
            Some(mut begin) => {
                begin += 3;
                let end = match subject[begin..].find(",") {
                    Some(e) => begin + e,
                    None => subject.len(),
                };
                &subject[begin..end]
            }
            None => {
                log::warn!("Subject missing CN= & OU= :{}", &subject);
                &subject
            }
        }
        .to_string();

        // grab a few arbitrary bytes from spki so pddb_key is deterministic & unique
        let k = &self.spki;
        pddb_key.push_str(&format!(" {:X}{:X}{:X}{:X}", k[6], k[7], k[8], k[9]));

        // mirror of pddb::KEY_NAME_LEN
        // u64: vaddr/len/resvd, u32: flags, age = 95
        // would this be better as a pddb pub?
        const KEY_NAME_LEN: usize = 127 - 8 - 8 - 8 - 4 - 4;
        pddb_key[..min(pddb_key.len(), KEY_NAME_LEN - 1)].to_string()
    }

    // decoded subject
    pub fn subject(&self) -> String {
        match x509_parser::x509::X509Name::from_der(&self.subject) {
            Ok((_, decoded)) => decoded.to_string(),
            Err(e) => {
                log::warn!("{:?}", e);
                "der decode failed".to_string()
            }
        }
    }
}

impl fmt::Display for OwnedTrustAnchor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.subject())
    }
}

impl<'a> From<TrustAnchor<'a>> for OwnedTrustAnchor {
    fn from(ta: TrustAnchor) -> Self {
        Self::from_subject_spki_name_constraints(
            ta.subject.as_ref().to_owned(),
            ta.subject_public_key_info.as_ref().to_owned(),
            ta.name_constraints.map(|nc| nc.as_ref().to_owned()),
        )
    }
}

impl<'a> From<&X509Certificate<'a>> for OwnedTrustAnchor {
    fn from(x509: &X509Certificate) -> Self {
        Self::from_subject_spki_name_constraints(
            x509.subject().as_raw(),
            x509.public_key().raw,
            None::<&[u8]>, // ignore name constraints for now TODO
        )
    }
}

impl<'a> Into<TrustAnchor<'a>> for OwnedTrustAnchor {
    fn into(self) -> TrustAnchor<'a> {
        TrustAnchor {
            subject: Der::from(self.subject),
            subject_public_key_info: Der::from(self.spki),
            name_constraints: self.name_constraints.map_or(None, |nc| Some(Der::from(nc))),
        }
        .to_owned()
    }
}
