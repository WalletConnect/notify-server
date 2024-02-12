use {
    super::assert_successful_response,
    data_encoding::BASE64URL,
    notify_server::{
        auth::DidWeb,
        rpc::decode_key,
        services::public_http_server::{
            handlers::{
                did_json::{DidJson, WC_NOTIFY_AUTHENTICATION_KEY_ID, WC_NOTIFY_SUBSCRIBE_KEY_ID},
                subscribe_topic::{SubscribeTopicRequestBody, SubscribeTopicResponseBody},
            },
            DID_JSON_ENDPOINT,
        },
        utils::get_client_id,
    },
    relay_rpc::domain::{DecodedClientId, ProjectId},
    std::fmt::Display,
    url::Url,
};

pub async fn subscribe_topic<T>(
    project_id: &ProjectId,
    project_secret: T,
    app_domain: DidWeb,
    notify_server_url: &Url,
) -> (
    x25519_dalek::PublicKey,
    ed25519_dalek::VerifyingKey,
    DecodedClientId,
)
where
    T: Display,
{
    let response = assert_successful_response(
        reqwest::Client::new()
            .post(
                notify_server_url
                    .join(&format!("/{project_id}/subscribe-topic",))
                    .unwrap(),
            )
            .bearer_auth(project_secret)
            .json(&SubscribeTopicRequestBody {
                app_domain: app_domain.into_domain(),
            })
            .send()
            .await
            .unwrap(),
    )
    .await
    .json::<SubscribeTopicResponseBody>()
    .await
    .unwrap();

    let authentication = decode_key(&response.authentication_key).unwrap();
    let key_agreement = decode_key(&response.subscribe_key).unwrap();

    let key_agreement = x25519_dalek::PublicKey::from(key_agreement);
    let authentication = ed25519_dalek::VerifyingKey::from_bytes(&authentication).unwrap();
    let client_id = get_client_id(&authentication);
    (key_agreement, authentication, client_id)
}

pub async fn get_notify_did_json(
    notify_server_url: &Url,
) -> (x25519_dalek::PublicKey, DecodedClientId) {
    let did_json_url = notify_server_url.join(DID_JSON_ENDPOINT).unwrap();
    let did_json = reqwest::get(did_json_url)
        .await
        .unwrap()
        .json::<DidJson>()
        .await
        .unwrap();
    let key_agreement = &did_json
        .verification_method
        .iter()
        .find(|key| key.id.ends_with(WC_NOTIFY_SUBSCRIBE_KEY_ID))
        .unwrap()
        .public_key_jwk
        .x;
    let authentication = &did_json
        .verification_method
        .iter()
        .find(|key| key.id.ends_with(WC_NOTIFY_AUTHENTICATION_KEY_ID))
        .unwrap()
        .public_key_jwk
        .x;
    let key_agreement: [u8; 32] = BASE64URL
        .decode(key_agreement.as_bytes())
        .unwrap()
        .try_into()
        .unwrap();
    let authentication: [u8; 32] = BASE64URL
        .decode(authentication.as_bytes())
        .unwrap()
        .try_into()
        .unwrap();
    (
        x25519_dalek::PublicKey::from(key_agreement),
        // Better approach, but dependency versions conflict right now
        // DecodedClientId::from_key(
        //     ed25519_dalek::VerifyingKey::from_bytes(&authentication).unwrap(),
        // ),
        DecodedClientId(authentication),
    )
}
