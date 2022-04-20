#[allow(unused)]
#[cfg(not(target_arch = "wasm32"))]
use std::io::{ErrorKind, Read, Write};

use crate::hash::sha256_hash;

use aead::{Aead, NewAead};

#[cfg(not(target_arch = "wasm32"))]
use aead::stream::{DecryptorBE32, EncryptorBE32};

use aes_gcm::{Aes256Gcm, Key, Nonce};
use chacha20poly1305::XChaCha20Poly1305;

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use wasm_bindgen::prelude::*;
        type Result<T> = std::result::Result<T, JsError>;

        #[allow(unused)]
        fn try_or_err(result: aead::Result<Vec<u8>>) -> std::result::Result<Vec<u8>, JsError> {
            match result {
                Ok(data) => Ok(data),
                Err(_) => Err(JsError::new("Error")),
            }
        }
    } else {
        use anyhow::{bail, Result};
        fn try_or_err(result: aead::Result<Vec<u8>>) -> Result<Vec<u8>> {
            result.map_err(|e| anyhow::anyhow!(e))
        }
    }
}

/// Used to encrypt data with AES256-GCM using a 256bit key.
/// Note: If key is less than or greater than 256bits/32bytes, it will hash the key with sha256 with nonce being its salt
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn aes256gcm_encrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let nonce = crate::generate(12);

    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce.clone())),
    };

    let key = Key::from_slice(&e_key);
    let a_nonce = Nonce::from_slice(&nonce);

    let cipher = Aes256Gcm::new(key);
    let mut edata = try_or_err(cipher.encrypt(a_nonce, data))?;

    edata.extend(nonce);

    Ok(edata)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn aes256gcm_self_encrypt(data: &[u8]) -> Result<Vec<u8>> {
    let key = crate::generate(34);
    let mut data = aes256gcm_encrypt(&key, data)?;
    data.extend(key);
    Ok(data)
}

/// Used to decrypt data with AES256-GCM using a 256bit key.
/// Note: If key is less than or greater than 256bits/32bytes, it will hash the key with sha256 with nonce being its salt
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn aes256gcm_decrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let ExtractedData {
        extract_a,
        extract_b,
    } = extract_data_slice(data, 12);
    let nonce = extract_a;
    let payload = extract_b;

    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce.clone())),
    };

    let key = Key::from_slice(&e_key);
    let nonce = Nonce::from_slice(&nonce);

    let cipher = Aes256Gcm::new(key);
    try_or_err(cipher.decrypt(nonce, payload.as_slice()))
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn aes256gcm_self_decrypt(data: &[u8]) -> Result<Vec<u8>> {
    let ExtractedData {
        extract_a,
        extract_b,
    } = extract_data_slice(data, 34);
    let key = extract_a;
    let payload = extract_b;
    aes256gcm_decrypt(&key, &payload)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn aes256gcm_encrypt_stream(
    key: &[u8],
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let nonce = crate::generate(7);
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce.clone())),
    };
    let mut buffer = [0u8; 512];
    let key = Key::from_slice(&e_key);

    let cipher = Aes256Gcm::new(key);

    let mut stream = EncryptorBE32::from_aead(cipher, nonce.as_slice().into());
    writer.write_all(&nonce)?;

    // loop and read `reader`
    loop {
        match reader.read(&mut buffer) {
            Ok(512) => {
                let ciphertext = stream
                    .encrypt_next(buffer.as_slice())
                    .map_err(|err| anyhow::anyhow!(err))?;
                writer.write_all(&ciphertext)?;
            }
            Ok(read_count) => {
                let ciphertext = stream
                    .encrypt_last(&buffer[..read_count])
                    .map_err(|err| anyhow::anyhow!(err))?;
                writer.write_all(&ciphertext)?;
                break;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => bail!(e),
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn aes256gcm_self_encrypt_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let key = crate::generate(34);
    writer.write_all(&key)?;
    aes256gcm_encrypt_stream(&key, reader, writer)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn aes256gcm_decrypt_stream(
    key: &[u8],
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let mut nonce = vec![0u8; 7];

    reader.read_exact(&mut nonce)?;
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce.clone())),
    };
    let key = Key::from_slice(&e_key);
    let cipher = Aes256Gcm::new(key);

    // loop and read `reader`
    let mut stream = DecryptorBE32::from_aead(cipher, nonce.as_slice().into());
    let mut buffer = [0u8; 528];
    loop {
        match reader.read(&mut buffer) {
            Ok(528) => {
                let plaintext = stream
                    .decrypt_next(buffer.as_slice())
                    .map_err(|e| anyhow::anyhow!(e))?;

                writer.write_all(&plaintext)?
            }
            Ok(read_count) if read_count == 0 => break,
            Ok(read_count) => {
                let plaintext = stream
                    .decrypt_last(&buffer[..read_count])
                    .map_err(|e| anyhow::anyhow!(e))?;
                writer.write_all(&plaintext)?;
                break;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => bail!(e),
        };
    }
    writer.flush()?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn aes256gcm_self_decrypt_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let mut key = vec![0u8; 34];
    reader.read_exact(&mut key)?;
    aes256gcm_decrypt_stream(&key, reader, writer)
}

/// Used to encrypt data using XChaCha20Poly1305 with a 256bit key
/// Note: If key is less than or greater than 256bits/32bytes, it will hash the key with sha256 with nonce being its salt
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn xchacha20poly1305_encrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let nonce = crate::generate(24);

    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce.clone())),
    };

    let chacha = XChaCha20Poly1305::new(e_key.as_slice().into());
    let mut cipher = try_or_err(chacha.encrypt(nonce.as_slice().into(), data))?;

    cipher.extend(nonce);

    Ok(cipher)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn xchacha20poly1305_self_encrypt(data: &[u8]) -> Result<Vec<u8>> {
    let key = crate::generate(34);
    let mut data = xchacha20poly1305_encrypt(&key, data)?;
    data.extend(key);
    Ok(data)
}

/// Used to decrypt data using XChaCha20Poly1305 with a 256bit key
/// Note: If key is less than or greater than 256bits/32bytes, it will hash the key with sha256 with nonce being its salt
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn xchacha20poly1305_decrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let ExtractedData {
        extract_a,
        extract_b,
    } = extract_data_slice(data, 24);
    let nonce = extract_a;
    let payload = extract_b;
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce.to_vec())),
    };

    let chacha = XChaCha20Poly1305::new(e_key.as_slice().into());

    try_or_err(chacha.decrypt(nonce.as_slice().into(), payload.as_ref()))
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn xchacha20poly1305_self_decrypt(data: &[u8]) -> Result<Vec<u8>> {
    let ExtractedData {
        extract_a,
        extract_b,
    } = extract_data_slice(data, 34);
    let key = extract_a;
    let payload = extract_b;
    xchacha20poly1305_decrypt(&key, &payload)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn xchacha20poly1305_encrypt_stream(
    key: &[u8],
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let nonce = crate::generate(19);
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce.clone())),
    };
    let mut buffer = [0u8; 512];
    let chacha = XChaCha20Poly1305::new(e_key.as_slice().into());

    let mut stream = EncryptorBE32::from_aead(chacha, nonce.as_slice().into());
    // write nonce to the beginning of the stream
    writer.write_all(&nonce)?;

    // loop and read `reader`
    loop {
        match reader.read(&mut buffer) {
            Ok(512) => {
                let ciphertext = stream
                    .encrypt_next(buffer.as_slice())
                    .map_err(|err| anyhow::anyhow!(err))?;
                writer.write_all(&ciphertext)?;
            }
            Ok(read_count) => {
                let ciphertext = stream
                    .encrypt_last(&buffer[..read_count])
                    .map_err(|err| anyhow::anyhow!(err))?;
                writer.write_all(&ciphertext)?;
                break;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => bail!(e),
        }
    }
    writer.flush()?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn xchacha20poly1305_self_encrypt_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let key = crate::generate(34);
    writer.write_all(&key)?;
    xchacha20poly1305_encrypt_stream(&key, reader, writer)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn xchacha20poly1305_decrypt_stream(
    key: &[u8],
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let mut nonce = vec![0u8; 19];

    reader.read_exact(&mut nonce)?;
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce.clone())),
    };
    let chacha = XChaCha20Poly1305::new(e_key.as_slice().into());
    let mut stream = DecryptorBE32::from_aead(chacha, nonce.as_slice().into());
    let mut buffer = [0u8; 528];
    loop {
        match reader.read(&mut buffer) {
            Ok(528) => {
                let plaintext = stream
                    .decrypt_next(buffer.as_slice())
                    .map_err(|e| anyhow::anyhow!(e))?;

                writer.write_all(&plaintext)?
            }
            Ok(read_count) if read_count == 0 => break,
            Ok(read_count) => {
                let plaintext = stream
                    .decrypt_last(&buffer[..read_count])
                    .map_err(|e| anyhow::anyhow!(e))?;
                writer.write_all(&plaintext)?;
                break;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => bail!(e),
        };
    }
    writer.flush()?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn xchacha20poly1305_self_decrypt_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let mut key = vec![0u8; 34];
    reader.read_exact(&mut key)?;
    xchacha20poly1305_decrypt_stream(&key, reader, writer)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct ExtractedData {
    extract_a: Vec<u8>,
    extract_b: Vec<u8>,
}

impl ExtractedData {
    pub fn extract_a(&self) -> Vec<u8> {
        self.extract_a.clone()
    }
    pub fn extract_b(&self) -> Vec<u8> {
        self.extract_b.clone()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn extract_data_slice(data: &[u8], size: usize) -> ExtractedData {
    let extracted = &data[data.len() - size..];
    let payload = &data[..data.len() - size];
    ExtractedData {
        extract_a: extracted.to_vec(),
        extract_b: payload.to_vec(),
    }
}

#[cfg(test)]
mod test {
    use crate::cipher::*;

    #[test]
    fn aes256gcm_encrypt_decrypt() -> anyhow::Result<()> {
        let key = b"this is my secret cipher key!";
        let message = b"Hello, World!";

        let cipher = aes256gcm_encrypt(key, message)?;

        let plaintext = aes256gcm_decrypt(key, &cipher)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[test]
    fn aes256gcm_self_encrypt_decrypt() -> anyhow::Result<()> {
        let message = b"Hello, World!";

        let cipher = aes256gcm_self_encrypt(message)?;

        let plaintext = aes256gcm_self_decrypt(&cipher)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_self_encrypt_decrypt() -> anyhow::Result<()> {
        let message = b"Hello, World!";

        let cipher = xchacha20poly1305_self_encrypt(message)?;

        let plaintext = xchacha20poly1305_self_decrypt(&cipher)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[test]
    fn aes256gcm_stream_self_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message";
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        aes256gcm_self_encrypt_stream(&mut base.as_slice(), &mut cipher)?;

        aes256gcm_self_decrypt_stream(&mut cipher.as_slice(), &mut plaintext)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }

    #[test]
    fn aes256gcm_stream_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message";
        let key = b"this is my key";
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        aes256gcm_encrypt_stream(key, &mut base.as_slice(), &mut cipher)?;

        aes256gcm_decrypt_stream(key, &mut cipher.as_slice(), &mut plaintext)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_encrypt_decrypt() -> anyhow::Result<()> {
        let key = b"this is my secret cipher key!";
        let message = b"Hello, World!";

        let cipher = xchacha20poly1305_encrypt(key, message)?;

        let plaintext = xchacha20poly1305_decrypt(key, &cipher)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_stream_encrypt_decrypt() -> anyhow::Result<()> {
        let key = b"this is my key";
        let base = b"this is my message";
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        xchacha20poly1305_encrypt_stream(key, &mut base.as_slice(), &mut cipher)?;

        xchacha20poly1305_decrypt_stream(key, &mut cipher.as_slice(), &mut plaintext)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_stream_self_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message";
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        xchacha20poly1305_self_encrypt_stream(&mut base.as_slice(), &mut cipher)?;

        xchacha20poly1305_self_decrypt_stream(&mut cipher.as_slice(), &mut plaintext)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }
}
