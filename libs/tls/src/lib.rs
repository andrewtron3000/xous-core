pub mod cmd;
pub mod danger;
pub mod rota;
pub mod trusted;

use crate::rota::RustlsOwnedTrustAnchor;
use locales::t;
use modals::Modals;
use rkyv::{
    de::deserializers::AllocDeserializer,
    ser::{serializers::WriteSerializer, Serializer},
    AlignedVec, Deserialize
};
use rustls::Certificate;
use std::convert::{Into, TryFrom};
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write};
use std::path::PathBuf;
use x509_parser::prelude::{FromDer, X509Certificate};
use xous_names::XousNames;

/// PDDB Dict for tls trusted certificates keys
const TLS_TRUSTED_DICT: &str = "tls.trusted";
const CURRENT_VERSION_KEY: &str = "__version";

// presents a modal to the user to select trusted tls certificates
// and saves the selected certificates to the pddb
// returns a count of trusted certificates
pub fn check_trust(certificates: &[Certificate]) -> usize {
    let xns = XousNames::new().unwrap();
    let modals = Modals::new(&xns).unwrap();

    let certificates: Vec<X509Certificate> = certificates
        .iter()
        .map(|cert| X509Certificate::from_der(cert.as_ref()))
        .filter(|result| result.is_ok())
        .map(|result| result.unwrap().1)
        .filter(|x509| x509.is_ca())
        .collect();

    let chain: Vec<String> = certificates
        .iter()
        .map(|x509| {
            let subject = x509.subject();
            format!(
                "{}{}\n{}\n{}",
                if x509.is_ca() { "🏛 " } else { "" },
                &subject,
                &x509.raw_serial_as_string()[0..24],
                &x509.raw_serial_as_string()[24..],
            )
        })
        .collect();
    let chain: Vec<&str> = chain.iter().map(AsRef::as_ref).collect();
    modals
        .add_list(chain)
        .expect("couldn't build checkbox list");
    match modals.get_checkbox(t!("tls.check_trust_prompt", locales::LANG)) {
        Ok(trusted) => {
            trusted
                .iter()
                .for_each(|cert| log::info!("trusts {}", cert));
            modals
                .get_check_index()
                .unwrap()
                .iter()
                .map(|i| &certificates[*i])
                .map(|x509| {
                    (
                        x509.raw_serial_as_string(),
                        RustlsOwnedTrustAnchor::from(x509),
                    )
                })
                .for_each(|(key, val)| {
                    save_cert(&key, &val).unwrap_or_else(|e| {
                        log::warn!("failed to save cert: {e}");
                        modals
                            .show_notification(
                                format!("failed to save: {}", &val.subject()).as_str(),
                                None,
                            )
                            .expect("modal failed");
                    });
                });
            trusted.len()
        }
        _ => {
            log::error!("get_checkbox failed");
            0
        }
    }
}


// deletes ALL tls trust-anchors from the pddb
// returns the number of certs deleted
pub fn del_all_cert() -> Result<usize, Error> {
    let mut count = 0;
    let mut keypath = PathBuf::new();
    keypath.push(TLS_TRUSTED_DICT);
    if std::fs::metadata(&keypath).is_ok() {
        for entry in std::fs::read_dir(keypath)? {
            let entry = entry?;
            let key = entry.file_name().into_string().unwrap();
            let path = entry.path();
            match std::fs::remove_file(path) {
                Ok(_) => {
                    log::info!("deleted {key}");
                    count += 1;
                }
                Err(e) => log::warn!("failed to delete {key} : {e}"),
            };
        };
    };
    Ok(count)
}


// deletes a tls trust-anchor from the pddb
pub fn del_cert(key: &str) -> Result<(), Error> {
    let mut keypath = PathBuf::new();
    keypath.push(TLS_TRUSTED_DICT);
    keypath.push(key);
    match std::fs::remove_file(keypath) {
                Ok(_) => log::info!("deleted {key}") ,
                Err(e) => log::warn!("failed to delete {key} : {e}"),
    };
    return Ok(());
}
// saves a tls trust-anchor to the pddb
pub fn save_cert(key: &str, ta: &RustlsOwnedTrustAnchor) -> Result<(), Error> {
    if key.starts_with("__") {
        Err(Error::new(
            ErrorKind::PermissionDenied,
            "may not set a variable beginning with __ ",
        ))
    } else {
        log::trace!("set '{}' = '{:?}'", key, ta);
        let mut keypath = PathBuf::new();
        keypath.push(TLS_TRUSTED_DICT);
        if !std::fs::metadata(&keypath).is_ok() {
            log::info!("dict '{}' does NOT exist.. creating it", TLS_TRUSTED_DICT);
            std::fs::create_dir_all(&keypath)?;
        }
        keypath.push(key);

        // serialize the trust-anchor
        let mut serializer = WriteSerializer::new(AlignedVec::new());
        let pos: u16 = u16::try_from(serializer.serialize_value(ta).unwrap()).expect("data > u16");

        // this next bit of black-magic bolts pos onto buff to enable deserialization
        let mut buf = serializer.into_inner();
        let pos_bytes = pos.to_be_bytes();
        buf.push(pos_bytes[0]);
        buf.push(pos_bytes[1]);

        // save key & trust-anchor to pddb
        File::create(keypath)?.write_all(&buf[..])?;
        Ok(())
    }
}

// retrieves a tls trust-anchor from the pddb
pub fn get_cert(key: &str) -> Result<Option<RustlsOwnedTrustAnchor>, Error> {
    let mut keypath = PathBuf::new();
    keypath.push(TLS_TRUSTED_DICT);
    if !std::fs::metadata(&keypath).is_ok() {
        log::info!("dict '{}' does NOT exist.. creating it", TLS_TRUSTED_DICT);
        std::fs::create_dir_all(&keypath)?;
    }

    keypath.push(key);
    if let Ok(mut file) = File::open(keypath) {
        // read the key & serialized trust-anchor from the pddb
        let mut bytes = Vec::<u8>::new();
        let len = file.read_to_end(&mut bytes)?;

        // black unmagic to unbold pos from the buffer end
        let pos: u16 = u16::from_be_bytes([bytes[len - 2], bytes[len - 1]]);

        // deserialize the trust-anchor
        let archive = unsafe { rkyv::archived_value::<RustlsOwnedTrustAnchor>(&bytes, pos.into()) };
        let ta = archive.deserialize(&mut AllocDeserializer {}).ok();

        log::trace!("get '{}' = '{:?}'", key, ta);
        return Ok(ta);
    } else {
        return Ok(None);
    }
}
