use rcgen::{CertificateParams, KeyPair};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CertError {
    #[error("certificate generation failed: {0}")]
    Generation(#[from] rcgen::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid certificate DER")]
    InvalidDer,
    #[error("invalid key DER")]
    InvalidKey,
}

pub struct Identity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl Identity {
    pub fn generate() -> Result<Self, CertError> {
        let key_pair = KeyPair::generate()?;
        let params = CertificateParams::new(vec!["vesa".to_string()])?;
        let cert = params.self_signed(&key_pair)?;

        Ok(Self {
            cert_der: cert.der().to_vec(),
            key_der: key_pair.serialize_der(),
        })
    }

    pub fn save(&self, dir: &Path) -> Result<(), CertError> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("cert.der"), &self.cert_der)?;
        std::fs::write(dir.join("key.der"), &self.key_der)?;
        Ok(())
    }

    pub fn load(dir: &Path) -> Result<Self, CertError> {
        let cert_der = std::fs::read(dir.join("cert.der"))?;
        let key_der = std::fs::read(dir.join("key.der"))?;
        Ok(Self { cert_der, key_der })
    }

    pub fn load_or_generate(dir: &Path) -> Result<Self, CertError> {
        match Self::load(dir) {
            Ok(identity) => Ok(identity),
            Err(_) => {
                let identity = Self::generate()?;
                identity.save(dir)?;
                Ok(identity)
            }
        }
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        let digest = ring::digest::digest(&ring::digest::SHA256, &self.cert_der);
        let mut result = [0u8; 32];
        result.copy_from_slice(digest.as_ref());
        result
    }

    pub fn rustls_cert(&self) -> rustls::pki_types::CertificateDer<'static> {
        rustls::pki_types::CertificateDer::from(self.cert_der.clone())
    }

    pub fn rustls_key(&self) -> Result<rustls::pki_types::PrivateKeyDer<'static>, CertError> {
        Ok(rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(self.key_der.clone()),
        ))
    }
}
