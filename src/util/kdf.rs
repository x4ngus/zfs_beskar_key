// ============================================================================
// src/util/kdf.rs – Minimal PBKDF2-HMAC-SHA256 implementation
// ============================================================================

use sha2::{Digest, Sha256};

pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    assert!(iterations >= 1, "iterations must be >= 1");
    let mut counter = 1u32;
    let mut generated = 0usize;

    while generated < out.len() {
        let block = pbkdf2_block(password, salt, iterations, counter);
        let take = (out.len() - generated).min(block.len());
        out[generated..generated + take].copy_from_slice(&block[..take]);
        generated += take;
        counter = counter.saturating_add(1);
    }
}

fn pbkdf2_block(password: &[u8], salt: &[u8], iterations: u32, counter: u32) -> [u8; 32] {
    let mut salt_counter = Vec::with_capacity(salt.len() + 4);
    salt_counter.extend_from_slice(salt);
    salt_counter.extend_from_slice(&counter.to_be_bytes());

    let mut u = hmac_sha256(password, &salt_counter);
    let mut result = u;

    for _ in 1..iterations {
        u = hmac_sha256(password, &u);
        for i in 0..32 {
            result[i] ^= u[i];
        }
    }

    result
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let digest = Sha256::digest(key);
        key_block[..32].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(inner_hash);
    let digest = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}
