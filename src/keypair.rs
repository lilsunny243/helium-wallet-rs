use crate::{
    mnemonic::entropy_to_mnemonic,
    result::{bail, Result},
    traits::ReadWrite,
};
use byteorder::ReadBytesExt;
use std::{convert::TryFrom, io};

pub use helium_crypto::{
    ecc_compact, ed25519, KeyTag, KeyType, Network, PublicKey, Sign, Verify, KEYTYPE_ED25519_STR,
    NETTYPE_MAIN_STR,
};

#[derive(PartialEq, Debug)]
pub struct Keypair(helium_crypto::Keypair);

static START: std::sync::Once = std::sync::Once::new();

fn init() {
    START.call_once(|| sodiumoxide::init().expect("Failed to intialize sodium"))
}

impl Default for Keypair {
    fn default() -> Self {
        Self::generate(KeyTag::default())
    }
}

impl Keypair {
    pub fn generate(key_tag: KeyTag) -> Self {
        use rand::rngs::OsRng;
        Keypair(helium_crypto::Keypair::generate(key_tag, &mut OsRng))
    }

    pub fn generate_from_entropy(key_tag: KeyTag, entropy: &[u8]) -> Result<Self> {
        Ok(Keypair(helium_crypto::Keypair::generate_from_entropy(
            key_tag, entropy,
        )?))
    }

    pub fn public_key(&self) -> &PublicKey {
        self.0.public_key()
    }

    pub fn sign(&self, msg: &[u8]) -> Result<Vec<u8>> {
        Ok(self.0.sign(msg)?)
    }

    /// Return the mnemonic phrase that can be used to recreate this Keypair.
    /// This function is implemented here to avoid passing the secret between
    /// too many modules.
    pub fn phrase(&self) -> Result<Vec<String>> {
        let entropy = self.0.secret_to_vec();
        entropy_to_mnemonic(&entropy)
    }

    /// Extract the underlying seed. We only support this method for ED25519
    /// since we provide this functionality for exporting seed to Solana CLI.
    pub fn unencrypted_seed(&self) -> Result<Vec<u8>> {
        match &self.0 {
            helium_crypto::Keypair::Ed25519(key) => {
                // Note we strip the leading helium type byte. What remains is a
                // standard ed25519 private key (secret followed by public key)
                Ok(key.to_vec()[1..].to_vec())
            }
            helium_crypto::Keypair::EccCompact(_) => {
                bail!("EccCompact key type unsupported for unencrypted seed write.")
            }
            helium_crypto::Keypair::Secp256k1(_) => {
                bail!("Secp256k1 key type unsupported for unencrypted seed write.")
            }
        }
    }
}

impl ReadWrite for Keypair {
    fn write(&self, writer: &mut dyn io::Write) -> Result {
        match &self.0 {
            helium_crypto::Keypair::Ed25519(key) => {
                writer.write_all(&key.to_vec())?;
                writer.write_all(&key.public_key.to_vec())?;
            }
            helium_crypto::Keypair::EccCompact(key) => {
                writer.write_all(&key.to_vec())?;
                writer.write_all(&key.public_key.to_vec())?;
            }
            helium_crypto::Keypair::Secp256k1(_) => {
                bail!("Secp256k1 key type unsupported for write.")
            }
        }
        Ok(())
    }

    fn read(reader: &mut dyn io::Read) -> Result<Keypair> {
        init();
        let tag = reader.read_u8()?;
        match KeyType::try_from(tag)? {
            KeyType::Ed25519 => {
                let mut sk_buf = [0u8; ed25519::KEYPAIR_LENGTH];
                sk_buf[0] = tag;
                reader.read_exact(&mut sk_buf[1..])?;
                Ok(Keypair(ed25519::Keypair::try_from(&sk_buf[..])?.into()))
            }
            KeyType::EccCompact => {
                let mut sk_buf = [0u8; ecc_compact::KEYPAIR_LENGTH];
                sk_buf[0] = tag;
                reader.read_exact(&mut sk_buf[1..])?;
                Ok(Keypair(ecc_compact::Keypair::try_from(&sk_buf[..])?.into()))
            }
            KeyType::MultiSig => Err(helium_crypto::Error::invalid_keytype(tag).into()),
            KeyType::Secp256k1 => bail!("Secp256k1 key type unsupported for read."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, str::FromStr};

    #[test]
    fn roundtrip_keypair() {
        let keypair = Keypair::default();
        let mut buffer = Vec::new();
        keypair
            .write(&mut buffer)
            .expect("Failed to encode keypair");

        let decoded = Keypair::read(&mut Cursor::new(buffer)).expect("Failed to decode keypair");
        assert_eq!(keypair, decoded);
    }

    #[test]
    fn roundtrip_public_key() {
        let pk = Keypair::default();
        let mut buffer = Vec::new();
        pk.public_key()
            .write(&mut buffer)
            .expect("Failed to encode public key");

        let decoded =
            PublicKey::read(&mut Cursor::new(buffer)).expect("Failed to decode public key");
        assert_eq!(pk.public_key(), &decoded);
    }

    #[test]
    fn roundtrip_b58_public_key() {
        let pk = Keypair::default();
        let decoded =
            PublicKey::from_str(&pk.public_key().to_string()).expect("Failed to decode public key");
        assert_eq!(pk.public_key(), &decoded);
    }

    #[test]
    fn test_seed_output() {
        let pk = Keypair::default();
        let seed = pk.unencrypted_seed().expect("ed25519 keypair seed");
        assert_eq!(64, seed.len());
    }
}
