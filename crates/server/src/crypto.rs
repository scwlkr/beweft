use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use rand::RngCore;
use sha2::{Digest, Sha256};
pub fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
pub fn digest(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
pub fn secrets_equal(a: &str, b: &str) -> bool {
    let a = Sha256::digest(a.as_bytes());
    let b = Sha256::digest(b.as_bytes());
    a.iter().zip(b).fold(0u8, |v, (a, b)| v | (a ^ b)) == 0
}
/// Bind credentials to their channel ID. The key is supplied separately from database backups.
pub fn encrypt(key: &[u8; 32], channel_id: uuid::Uuid, value: &str) -> anyhow::Result<String> {
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let cipher = ChaCha20Poly1305::new(key.into());
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: value.as_bytes(),
                aad: channel_id.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("credential encryption failed"))?;
    let mut result = nonce.to_vec();
    result.extend(encrypted);
    Ok(STANDARD.encode(result))
}
pub fn decrypt(key: &[u8; 32], channel_id: uuid::Uuid, value: &str) -> anyhow::Result<String> {
    let decoded = STANDARD.decode(value)?;
    anyhow::ensure!(decoded.len() >= 28, "invalid credential envelope");
    let cipher = ChaCha20Poly1305::new(key.into());
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&decoded[..12]),
            Payload {
                msg: &decoded[12..],
                aad: channel_id.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("credential decryption failed"))?;
    Ok(String::from_utf8(plain)?)
}
