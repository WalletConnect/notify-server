use {
    crate::error::Result,
    walletconnect_sdk::rpc::{
        auth::{ed25519_dalek::Keypair, AuthToken},
        domain::{ClientId, DecodedClientId},
    },
};

pub fn jwt_token(url: &str, keypair: &Keypair) -> Result<String> {
    let decoded_client_id = DecodedClientId(*keypair.public_key().as_bytes());
    let client_id = ClientId::from(decoded_client_id);

    AuthToken::new(client_id.value().clone())
        .aud(url)
        .as_jwt(keypair)
        .map(|x| x.to_string())
        .map_err(|e| e.into())
}
