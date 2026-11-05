use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    date_time_ymd,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tempfile::TempDir;

use crate::SetupError;

pub const CA_FILE: &str = "test-ca.pem";

/// A certificate chain and key for a test server.
pub struct ServerIdentity {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}

/// A CA generated at test time, written to `CA_FILE` in its own directory so
/// endpoint files can name it.
pub struct TestPki {
    pub directory: TempDir,
    ca_params: CertificateParams,
    ca_key: KeyPair,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Validity {
    Current,
    Expired,
}

fn ca(common_name: &str) -> Result<(CertificateParams, KeyPair, String), SetupError> {
    let key = KeyPair::generate()?;
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    params.distinguished_name = name;
    let pem = params.self_signed(&key)?.pem();
    Ok((params, key, pem))
}

impl TestPki {
    pub fn new() -> Result<Self, SetupError> {
        let directory = tempfile::tempdir()?;
        let (ca_params, ca_key, pem) = ca("snet test CA")?;
        std::fs::write(directory.path().join(CA_FILE), pem)?;
        Ok(Self {
            directory,
            ca_params,
            ca_key,
        })
    }

    pub fn ca_path(&self) -> std::path::PathBuf {
        self.directory.path().join(CA_FILE)
    }

    pub fn server_identity(
        &self,
        names: &[&str],
        validity: Validity,
    ) -> Result<ServerIdentity, SetupError> {
        issue(&self.ca_params, &self.ca_key, names, validity)
    }

    /// Signed by a CA that no endpoint file names.
    pub fn untrusted_identity(&self, names: &[&str]) -> Result<ServerIdentity, SetupError> {
        let (params, key, _pem) = ca("Unknown CA")?;
        issue(&params, &key, names, Validity::Current)
    }
}

fn issue(
    ca_params: &CertificateParams,
    ca_key: &KeyPair,
    names: &[&str],
    validity: Validity,
) -> Result<ServerIdentity, SetupError> {
    let key = KeyPair::generate()?;
    let names: Vec<String> = names.iter().map(|name| name.to_string()).collect();
    let mut params = CertificateParams::new(names)?;
    if validity == Validity::Expired {
        params.not_before = date_time_ymd(2020, 1, 1);
        params.not_after = date_time_ymd(2021, 1, 1);
    }
    let issuer = Issuer::from_params(ca_params, ca_key);
    let certificate = params.signed_by(&key, &issuer)?;
    Ok(ServerIdentity {
        chain: vec![certificate.der().clone()],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
    })
}

/// A certificate reader for an endpoint's CA names, over a test's own
/// directory; the product reads them through `snet-files`.
pub fn read_from(
    directory: &std::path::Path,
) -> impl Fn(&std::path::Path) -> Result<Vec<u8>, String> + '_ {
    move |name| std::fs::read(directory.join(name)).map_err(|error| error.to_string())
}
