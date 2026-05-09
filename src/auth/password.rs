use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

fn hasher() -> Argon2<'static> {
    // OWASP-recommended params for argon2id (2024): memory=19 MiB, t=2, p=1
    let params = Params::new(19_456, 2, 1, None).expect("valid argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let h = hasher()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash error: {e}"))?;
    Ok(h.to_string())
}

pub fn verify(password: &str, phc: &str) -> bool {
    let parsed = match PasswordHash::new(phc) {
        Ok(h) => h,
        Err(_) => return false,
    };
    hasher()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let phc = hash("hunter2-foobar").unwrap();
        assert!(verify("hunter2-foobar", &phc));
        assert!(!verify("hunter2-foobaR", &phc));
    }
}
