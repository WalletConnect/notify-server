use {
    base64::Engine,
    chrono::Utc,
    ed25519_dalek::{Keypair, PublicKey, Signer},
    jsonwebtoken::{Algorithm, Header},
    serde::{Deserialize, Serialize},
};

#[derive(Deserialize, Serialize)]
pub struct Claims {
    iss: String,
    sub: String,
    aud: String,
    iat: i64,
    exp: i64,
}

impl Claims {
    fn new(url: &str, pubkey: PublicKey) -> Self {
        const PREFIX_LEN: usize = MULTICODEC_ED25519_HEADER.len();
        const TOTAL_LEN: usize = MULTICODEC_ED25519_LENGTH + PREFIX_LEN;

        let mut prefixed_data: [u8; TOTAL_LEN] = [0; TOTAL_LEN];
        prefixed_data[..PREFIX_LEN].copy_from_slice(&MULTICODEC_ED25519_HEADER);
        prefixed_data[PREFIX_LEN..].copy_from_slice(&pubkey.to_bytes());

        let encoded = bs58::encode(prefixed_data).into_string();

        Self {
            sub: "e9bdf6bc8323ea6983f5491916c43c6e3c25c021cbcedeae39bbbb34557c24ce".to_string(),
            iss: format!("did:key:{MULTICODEC_ED25519_BASE}{encoded}"),
            aud: url.to_string(),
            iat: Utc::now().timestamp() - 60 * 10,
            exp: Utc::now().timestamp() + 3600,
        }
    }
}
pub const MULTICODEC_ED25519_BASE: &str = "z";
pub const MULTICODEC_ED25519_HEADER: [u8; 2] = [237, 1];
pub const MULTICODEC_ED25519_LENGTH: usize = 32;

pub fn jwt_token(url: &str, key: &Keypair) -> String {
    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_string(&Claims::new(url, key.public)).unwrap());

    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_string(&Header::new(Algorithm::EdDSA)).unwrap());
    // let pem = base64::engine::general_purpose::STANDARD_NO_PAD.encode(key.secret.
    // to_bytes());
    // key.public()
    // let pem = base64::encode(key.secret.to_bytes());
    // let pem =
    // format!("-----BEGIN ED25519 PRIVATE KEY-----\n{pem}\n-----END ED25519 PRIVATE
    // KEY-----"); println!("{}", pem);
    // let key = EncodingKey::from_ed_pem(pem.as_bytes()).unwrap();
    // jsonwebtoken::encode(&header, &claims, &key).unwrap()
    let body = format!("{header}.{claims}");
    let signature =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.sign(body.as_bytes()));

    format!("{body}.{signature}")
}
