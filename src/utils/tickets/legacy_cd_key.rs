//! Legacy CD-key resolution. Source: `Utils/Tickets/LegacyCDKey.cpp`.

use crate::utils::config::lua_config;
use crate::utils::support::fnv::fnv1a_hash;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn synthesize(app_id: u32, account_id: u32) -> String {
    let seed_a = format!("{}:{}", app_id, account_id);
    let seed_b = format!("OST:{}:{}", account_id, app_id);

    let mut bits: u64 = ((fnv1a_hash(&seed_a) as u64) << 32) | fnv1a_hash(&seed_b) as u64;

    let mut key = String::with_capacity(19);
    for i in 0..16 {
        if i != 0 && i % 4 == 0 { key.push('-'); }
        key.push(ALPHABET[(bits & 31) as usize] as char);
        bits = (bits >> 4) | (bits << 60);
    }
    key
}

/// Resolve a legacy CD key for a managed app.
///
/// `None` means "not an app we manage" — callers must let Steam's real
/// flow run. `Some` means we have a value to serve (either a user
/// override or a synthesized one).
pub fn resolve(app_id: u32, account_id: u32) -> Option<String> {
    if !lua_config::has_depot(app_id, true) {
        LOG_TRACE!("LegacyCDKey: appId={} not managed, passing through", app_id);
        return None;
    }
    if let Some(override_) = lua_config::get_legacy_cd_key(app_id) {
        LOG_INFO!("LegacyCDKey: appId={} -> override", app_id);
        return Some(override_);
    }
    let synth = synthesize(app_id, account_id);
    LOG_INFO!("LegacyCDKey: appId={} -> synthesised {}", app_id, synth);
    Some(synth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesize_shape() {
        let k = synthesize(1234567, 99999);
        // 16 alphabet chars + 3 dashes
        assert_eq!(k.len(), 19);
        assert_eq!(&k[4..5], "-");
        assert_eq!(&k[9..10], "-");
        assert_eq!(&k[14..15], "-");
    }

    #[test]
    fn synthesize_is_deterministic() {
        let a = synthesize(100, 200);
        let b = synthesize(100, 200);
        assert_eq!(a, b);
    }

    #[test]
    fn synthesize_differs_by_app() {
        assert_ne!(synthesize(100, 200), synthesize(101, 200));
    }

    #[test]
    fn synthesize_differs_by_account() {
        assert_ne!(synthesize(100, 200), synthesize(100, 201));
    }

    #[test]
    fn synthesize_alphabet_is_crockford() {
        let k = synthesize(1, 1);
        for c in k.chars() {
            if c == '-' { continue; }
            assert!(ALPHABET.contains(&(c as u8)), "char {} not in alphabet", c);
        }
    }
}
