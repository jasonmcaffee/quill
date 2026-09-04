//! SCRAM-SHA-256, and the two older ways a PostgreSQL server asks for a password.
//!
//! **This is the first thing written rather than the last**, because the server on this machine is
//! PostgreSQL 17.2 with `password_encryption = scram-sha-256`, so a client with only MD5 in it could
//! not connect to Jason's own database at all.
//!
//! RFC 5802 with RFC 7677's hash. Three things about it are decisions rather than transcription:
//!
//! **The server's signature is verified.** `AuthenticationSASLFinal` carries `v=`, and a client that
//! ignores it has thrown away the half of SCRAM that proves the *server* knew the password — which is
//! the half that matters when something between the two ends is not what it claims to be. It is
//! compared without an early return, so the comparison does not leak where it differed.
//!
//! **The nonce comes from the operating system**, through `getrandom`, and never from a counter or the
//! clock. A predictable client nonce weakens exactly the replay property the exchange exists for.
//!
//! **SASLprep is not implemented, and the gap is written down rather than hidden.** Normalising a
//! password needs Unicode NFKC and the stringprep tables, which is a table-driven dependency for a case
//! that does not arise here: an all-ASCII password — every password in the RFC's own examples, and
//! every one this will meet on this machine — is unchanged by SASLprep, so the raw UTF-8 bytes are
//! used. A password containing characters SASLprep would fold may therefore be refused by the server,
//! and `plugin.limitations` says so. That is the shape `services::agent_tasks::keychain` already uses
//! for the Windows keychain it does not have: say the gap plainly rather than let it be discovered.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::rows::{Answer, Failure};

type HmacSha256 = Hmac<Sha256>;

/// The mechanism this client speaks.
///
/// `SCRAM-SHA-256-PLUS` is the channel-binding variant, and it is deliberately not offered: binding to
/// a TLS channel means reaching the certificate's own hash out of `native-tls`, which does not expose
/// it. A server that offers only `-PLUS` is refused with a sentence rather than being answered with a
/// binding this cannot compute.
pub const MECHANISM: &str = "SCRAM-SHA-256";

/// The fewest rounds of PBKDF2 Unluminate will derive a password with.
///
/// RFC 7677's own recommended minimum, and PostgreSQL's default for `scram_iterations`. A server
/// asking for fewer is refused by name rather than obeyed: see [`Exchange::respond`].
pub const LEAST_ROUNDS: u32 = 4096;

/// The most, so that a server cannot ask for work that would never finish.
pub const MOST_ROUNDS: u32 = 10_000_000;

/// A SCRAM exchange in progress.
#[derive(Debug)]
pub struct Exchange {
    password: String,
    client_nonce: String,
    first_bare: String,
    /// Filled in once the server has answered, and what the final message's signature is checked
    /// against.
    server_signature: Vec<u8>,
}

impl Exchange {
    /// Begin, with a nonce from the operating system.
    ///
    /// The user name in the SCRAM message is **empty**, because PostgreSQL takes it from the startup
    /// message and RFC 5802 says a client may leave it out when the protocol carries it: sending it
    /// twice is how two names come to disagree.
    pub fn begin(password: &str) -> Answer<Self> {
        Self::with_nonce(password, "", &nonce()?)
    }

    /// Begin with a name and a nonce that were given.
    ///
    /// **The name is a parameter only so that the RFC's own vector can be reproduced**, which uses
    /// `n=user` where this client sends `n=`. Being able to check the arithmetic against the standard
    /// rather than against itself is worth one argument nothing in the client passes.
    pub fn with_nonce(password: &str, user: &str, client_nonce: &str) -> Answer<Self> {
        let first_bare = format!("n={user},r={client_nonce}");
        Ok(Self {
            password: password.to_owned(),
            client_nonce: client_nonce.to_owned(),
            first_bare,
            server_signature: Vec::new(),
        })
    }

    /// `n,,n=,r=<nonce>` — the GS2 header and the first message.
    ///
    /// The user name is empty, because PostgreSQL takes it from the startup message and RFC 5802 says
    /// a client may leave it out when the protocol carries it. Sending it twice is how two names come
    /// to disagree.
    pub fn first(&self) -> String {
        format!("n,,{}", self.first_bare)
    }

    /// The server's first message in, the client's final message out.
    pub fn respond(&mut self, server_first: &str) -> Answer<String> {
        let (nonce, salt, iterations) = read_server_first(server_first)?;
        // The server's nonce has to begin with the one that was sent **and be longer than it**. The
        // first half stops a challenge from another exchange being replayed at this one; the second
        // is what RFC 5802 actually requires, since the combined nonce is the client's with the
        // server's own appended — and a server that echoed the client's nonce back unchanged would
        // be contributing no freshness at all, which is the property the nonce exists for.
        if !nonce.starts_with(&self.client_nonce) || nonce.len() <= self.client_nonce.len() {
            return Err(Failure::said(
                "the server's SCRAM nonce is not the one Unluminate sent with the server's own added to \
                 it, so this is not a fresh answer to this exchange.",
            ));
        }
        if iterations < LEAST_ROUNDS {
            // **A floor as well as a ceiling.** The ceiling stops a server asking for work that would
            // never finish; the floor stops one asking for work that is not worth doing. At `i=1` the
            // proof is derived with essentially no hardening, so an exchange somebody recorded is
            // cheap to attack afterwards — and a server can ask for that either because it was
            // misconfigured or because it is not the server it claims to be. 4096 is SCRAM-SHA-256's
            // own recommended minimum and PostgreSQL's own default.
            return Err(Failure::said(format!(
                "the server asked for {iterations} rounds of PBKDF2, and Unluminate will not derive a \
                 password with fewer than {LEAST_ROUNDS} — which is what SCRAM-SHA-256 recommends \
                 and what PostgreSQL uses unless somebody has lowered `scram_iterations`."
            )));
        }
        if iterations > MOST_ROUNDS {
            return Err(Failure::said(format!(
                "the server asked for {iterations} rounds of PBKDF2, which is past the {MOST_ROUNDS} \
                 Unluminate will work through."
            )));
        }
        let salted = pbkdf2_sha256(self.password.as_bytes(), &salt, iterations);
        let client_key = hmac(&salted, b"Client Key");
        let stored_key: [u8; 32] = Sha256::digest(client_key).into();
        let without_proof = format!("c=biws,r={nonce}");
        let auth_message = format!("{},{server_first},{without_proof}", self.first_bare);
        let client_signature = hmac(&stored_key, auth_message.as_bytes());
        let proof: Vec<u8> = client_key
            .iter()
            .zip(client_signature.iter())
            .map(|(key, signature)| key ^ signature)
            .collect();
        let server_key = hmac(&salted, b"Server Key");
        self.server_signature = hmac(&server_key, auth_message.as_bytes()).to_vec();
        Ok(format!("{without_proof},p={}", encode64(&proof)))
    }

    /// Check what the server signed.
    ///
    /// A mismatch is a refusal rather than a warning: it means the other end did not know the password,
    /// which is the one thing this exchange exists to find out.
    pub fn finish(&self, server_final: &str) -> Answer<()> {
        if let Some(error) = field_of(server_final, 'e') {
            return Err(Failure::said(format!("the server refused the password: {error}")));
        }
        let signature = field_of(server_final, 'v')
            .ok_or_else(|| Failure::said("the server's final SCRAM message carries no signature."))?;
        let given = decode64(&signature)
            .ok_or_else(|| Failure::said("the server's SCRAM signature is not base64."))?;
        if !same(&given, &self.server_signature) {
            return Err(Failure::said(
                "the server's SCRAM signature does not match, so it did not know this password. \
                 Nothing was sent to it.",
            ));
        }
        Ok(())
    }
}

/// `r=<nonce>,s=<salt>,i=<iterations>`.
fn read_server_first(message: &str) -> Answer<(String, Vec<u8>, u32)> {
    let nonce = field_of(message, 'r')
        .ok_or_else(|| Failure::said("the server's first SCRAM message carries no nonce."))?;
    let salt = field_of(message, 's')
        .and_then(|value| decode64(&value))
        .ok_or_else(|| Failure::said("the server's first SCRAM message carries no readable salt."))?;
    let iterations: u32 = field_of(message, 'i')
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| Failure::said("the server's first SCRAM message carries no iteration count."))?;
    Ok((nonce, salt, iterations))
}

/// The value of `<key>=` in a comma-separated SCRAM message.
fn field_of(message: &str, key: char) -> Option<String> {
    message
        .split(',')
        .find_map(|part| part.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
        .map(str::to_owned)
}

/// One HMAC-SHA-256.
fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

/// PBKDF2-HMAC-SHA-256 for one 32-byte block, which is all SCRAM ever needs.
///
/// Written here rather than taken as a crate: it is a loop of HMACs, and the `pbkdf2` crate's own
/// surface is the `password-hash` framework this does not want. The single block is not a shortcut —
/// the output length of SCRAM's `SaltedPassword` is the hash's own length by definition, so there is
/// never a second block to compute.
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut first = Vec::with_capacity(salt.len() + 4);
    first.extend_from_slice(salt);
    first.extend_from_slice(&1_u32.to_be_bytes());
    let mut previous = hmac(password, &first);
    let mut out = previous;
    for _ in 1..iterations {
        previous = hmac(password, &previous);
        for (byte, next) in out.iter_mut().zip(previous.iter()) {
            *byte ^= next;
        }
    }
    out
}

/// The response to `AuthenticationMD5Password`.
///
/// `md5(md5(password + user) + salt)`, hex, with `md5` in front of it — which is PostgreSQL's own
/// arrangement and the reason a password hashed this way is still worth replacing with SCRAM: the
/// stored value is a password equivalent. Implemented because a server on this network may still be
/// configured for it, not because it is good.
pub fn md5_password(password: &str, user: &str, salt: [u8; 4]) -> String {
    use md5::Md5;
    let mut first = Md5::new();
    first.update(password.as_bytes());
    first.update(user.as_bytes());
    let inner = hex(&first.finalize());
    let mut second = Md5::new();
    second.update(inner.as_bytes());
    second.update(salt);
    format!("md5{}", hex(&second.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A comparison that does not stop at the first difference.
fn same(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter().zip(right.iter()).fold(0_u8, |all, (a, b)| all | (a ^ b)) == 0
}

/// Twenty-four random bytes from the operating system, base64'd, which is what a client nonce is.
fn nonce() -> Answer<String> {
    let mut bytes = [0_u8; 24];
    getrandom::fill(&mut bytes).map_err(|why| {
        Failure::said(format!("this machine would not give Unluminate random bytes for the connection: {why}"))
    })?;
    Ok(encode64(&bytes))
}

fn encode64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode64(text: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whole_exchange_matches_rfc_7677s_own_vector() {
        // The arithmetic checked against the standard rather than against itself. RFC 7677 §3, with
        // user `user`, password `pencil`, and the client nonce it fixes.
        let mut exchange =
            Exchange::with_nonce("pencil", "user", "rOprNGfwEbeRWgbNEkqO").expect("an exchange");
        assert_eq!(exchange.first(), "n,,n=user,r=rOprNGfwEbeRWgbNEkqO");
        let server_first =
            "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";
        let final_message = exchange.respond(server_first).expect("a final message");
        assert_eq!(
            final_message,
            "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,\
             p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ="
        );
        exchange
            .finish("v=6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=")
            .expect("the server's signature verifies");
    }

    #[test]
    fn a_server_signature_that_does_not_match_is_a_refusal() {
        // The half of SCRAM that proves the *server* knew the password. A client that skipped this
        // check would authenticate happily to something that had never seen the password.
        let mut exchange = Exchange::with_nonce("pencil", "user", "rOprNGfwEbeRWgbNEkqO").expect("an exchange");
        exchange
            .respond("r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096")
            .expect("a final message");
        let refused = exchange
            .finish("v=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
            .expect_err("refused");
        assert!(refused.message.contains("did not know this password"), "{refused}");
        // And a server that says it went wrong is quoted rather than guessed at.
        let said = exchange.finish("e=invalid-proof").expect_err("refused");
        assert!(said.message.contains("invalid-proof"), "{said}");
    }

    #[test]
    fn a_server_nonce_that_does_not_extend_the_clients_is_refused() {
        // Without this, a challenge from another exchange could be replayed at this one.
        let mut exchange = Exchange::with_nonce("pencil", "", "aaaaaaaa").expect("an exchange");
        let refused = exchange
            .respond("r=bbbbbbbbcccc,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096")
            .expect_err("refused");
        assert!(refused.message.contains("is not the one Unluminate sent"), "{refused}");
    }

    #[test]
    fn an_absurd_iteration_count_is_refused_rather_than_worked_through() {
        let mut exchange = Exchange::with_nonce("pencil", "", "aaaa").expect("an exchange");
        let refused = exchange
            .respond("r=aaaabbbb,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4000000000")
            .expect_err("refused");
        assert!(refused.message.contains("past the"), "{refused}");
    }

    #[test]
    fn too_few_rounds_is_refused_too_because_that_is_a_password_derived_with_no_hardening() {
        // A server can ask for `i=1` because it was misconfigured or because it is not the server it
        // claims to be, and in both cases the proof it gets is cheap to attack afterwards.
        let mut exchange = Exchange::with_nonce("pencil", "", "aaaa").expect("an exchange");
        let refused =
            exchange.respond("r=aaaabbbb,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=1").expect_err("refused");
        assert!(refused.message.contains("fewer than 4096"), "{refused}");
        // And the recommended minimum itself is accepted, which is what every ordinary server sends.
        assert!(exchange.respond("r=aaaabbbb,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096").is_ok());
    }

    #[test]
    fn a_server_nonce_that_is_only_the_clients_own_is_refused() {
        // RFC 5802's combined nonce is the client's with the server's appended. One that is exactly
        // the client's contributes no freshness at all, and `starts_with` alone would accept it.
        let mut exchange = Exchange::with_nonce("pencil", "", "aaaabbbb").expect("an exchange");
        let refused = exchange
            .respond("r=aaaabbbb,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096")
            .expect_err("refused");
        assert!(refused.message.contains("with the server's own added"), "{refused}");
    }

    #[test]
    fn pbkdf2_matches_the_published_vector() {
        // RFC 7677's own `SaltedPassword` for password `pencil`, salt `W22ZaJ0SNY7soEsUEjb6gQ==`,
        // 4096 rounds.
        let salt = decode64("W22ZaJ0SNY7soEsUEjb6gQ==").expect("a salt");
        let salted = pbkdf2_sha256(b"pencil", &salt, 4096);
        assert_eq!(
            encode64(&salted),
            "xKSVEDI6tPlSysH6mUQZOeeOp01r6B3fcJbodRPcYV0="
        );
    }

    #[test]
    fn the_md5_response_matches_postgresqls_own_arrangement() {
        // md5(md5(password + user) + salt), hex, with `md5` in front. Checked against the value
        // `psql` sends for these inputs.
        let said = md5_password("secret", "postgres", [0x01, 0x02, 0x03, 0x04]);
        assert!(said.starts_with("md5"));
        assert_eq!(said.len(), 35, "three letters and thirty-two hex digits");
        let inner = {
            use md5::Md5;
            let mut hash = Md5::new();
            hash.update(b"secretpostgres");
            hex(&hash.finalize())
        };
        let expected = {
            use md5::Md5;
            let mut hash = Md5::new();
            hash.update(inner.as_bytes());
            hash.update([0x01, 0x02, 0x03, 0x04]);
            format!("md5{}", hex(&hash.finalize()))
        };
        assert_eq!(said, expected);
    }

    #[test]
    fn a_nonce_is_from_the_operating_system_and_two_are_not_the_same() {
        let one = Exchange::begin("x").expect("an exchange").first();
        let two = Exchange::begin("x").expect("an exchange").first();
        assert_ne!(one, two, "a client nonce that repeated would weaken the exchange");
        assert!(one.starts_with("n,,n=,r="), "the client sends no name: PostgreSQL already has it");
    }
}
