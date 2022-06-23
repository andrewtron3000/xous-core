use crypto_common::InvalidLength;
use sha1::Sha1;
use hmac::{Hmac, Mac};
use digest::Update;
use std::{
    convert::TryFrom,
    time::{SystemTime, SystemTimeError},
};

// Derived from https://github.com/blakesmith/xous-core/blob/xtotp-time/apps/xtotp/src/main.rs
#[derive(Clone, Copy)]
pub(crate) enum TotpAlgorithm {
    HmacSha1,
    HmacSha256,
    HmacSha512,
}
impl std::fmt::Debug for TotpAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TotpAlgorithm::HmacSha1 => write!(f, "SHA1"),
            TotpAlgorithm::HmacSha256 => write!(f, "SHA256"),
            TotpAlgorithm::HmacSha512 => write!(f, "SHA512"),
        }
    }
}

impl TryFrom<&str> for TotpAlgorithm {
    type Error = xous::Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "SHA1" => Ok(TotpAlgorithm::HmacSha1),
            "SHA256" => Ok(TotpAlgorithm::HmacSha256),
            "SHA512" => Ok(TotpAlgorithm::HmacSha512),
            _ => Err(xous::Error::InvalidString)
        }
    }
}
impl Into<String> for TotpAlgorithm {
    fn into(self) -> String {
        match self {
            TotpAlgorithm::HmacSha1 => "SHA1".to_string(),
            TotpAlgorithm::HmacSha256 => "SHA256".to_string(),
            TotpAlgorithm::HmacSha512 => "SHA512".to_string(),
        }
    }
}

#[derive(Debug)]
struct TotpEntry {
    name: String,
    step_seconds: u16,
    shared_secret: Vec<u8>,
    digit_count: u8,
    algorithm: TotpAlgorithm,
}

#[derive(Debug)]
enum Error {
    Io(std::io::Error),
    DigestLength(InvalidLength),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<InvalidLength> for Error {
    fn from(err: InvalidLength) -> Self {
        Error::DigestLength(err)
    }
}

fn get_current_unix_time() -> Result<u64, SystemTimeError> {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
}

fn unpack_u64(v: u64) -> [u8; 8] {
    let mask = 0x00000000000000ff;
    let mut bytes: [u8; 8] = [0; 8];
    (0..8).for_each(|i| bytes[7 - i] = (mask & (v >> (i * 8))) as u8);
    bytes
}

fn generate_hmac_bytes(unix_timestamp: u64, totp_entry: &TotpEntry) -> Result<Vec<u8>, Error> {
    let mut computed_hmac = Vec::new();
    match totp_entry.algorithm {
        // The OpenTitan HMAC core does not support hmac-sha1. Fall back to
        // a software implementation.
        TotpAlgorithm::HmacSha1 => {
            let mut mac: Hmac<Sha1> = Hmac::new_from_slice(&totp_entry.shared_secret)?;
            mac.update(&unpack_u64(unix_timestamp / totp_entry.step_seconds as u64));
            let hash: &[u8] = &mac.finalize().into_bytes();
            computed_hmac.extend_from_slice(hash);
        }
        _ => todo!(),
    }

    Ok(computed_hmac)
}

fn generate_totp_code(unix_timestamp: u64, totp_entry: &TotpEntry) -> Result<String, Error> {
    let hash = generate_hmac_bytes(unix_timestamp, totp_entry)?;
    let offset: usize = (hash.last().unwrap_or(&0) & 0xf) as usize;
    let binary: u64 = (((hash[offset] & 0x7f) as u64) << 24)
        | ((hash[offset + 1] as u64) << 16)
        | ((hash[offset + 2] as u64) << 8)
        | (hash[offset + 3] as u64);

    let truncated_code = format!(
        "{:01$}",
        binary % (10_u64.pow(totp_entry.digit_count as u32)),
        totp_entry.digit_count as usize
    );

    Ok(truncated_code)
}