//! The minimum HTTPS client the well-known transport needs.

use crate::error::{Error, Result};

const USER_AGENT: &str = concat!("skill-manager/", env!("CARGO_PKG_VERSION"));

pub fn get(url: &str) -> Result<Vec<u8>> {
    let mut response = ureq::get(url)
        .header("user-agent", USER_AGENT)
        .call()
        .map_err(|error| Error::Network(format!("GET {url}: {error}")))?;
    response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|error| Error::Network(format!("GET {url}: {error}")))
}

pub fn get_string(url: &str) -> Result<String> {
    String::from_utf8(get(url)?)
        .map_err(|_| Error::Network(format!("GET {url}: response is not valid UTF-8")))
}
