use {
    crate::{extractors::AuthedProjectId, state::AppState},
    axum::{self, extract::State, response::IntoResponse, Json},
    chacha20poly1305::aead::{rand_core::RngCore, OsRng},
    log::info,
    mongodb::{bson::doc, options::ReplaceOptions},
    serde::{Deserialize, Serialize},
    serde_json::json,
    std::sync::Arc,
    x25519_dalek::{PublicKey, StaticSecret},
};

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectData {
    #[serde(rename = "_id")]
    pub id: String,
    pub identity_keypair: Keypair,
    pub signing_keypair: Keypair,
    pub dapp_url: String,
    pub topic: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Keypair {
    pub private_key: String,
    pub public_key: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeTopicData {
    dapp_url: String,
}

// TODO test idempotency

pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
    Json(subscribe_topic_data): Json<SubscribeTopicData>,
) -> Result<axum::response::Response, crate::error::Error> {
    info!("Generating keypair for project: {}", project_id);
    let db = state.database.clone();

    if let Some(project_data) = db
        .collection::<ProjectData>("project_data")
        .find_one(doc! { "_id": project_id.clone()}, None)
        .await?
        .iter()
        .next()
    {
        let signing_pubkey = project_data.signing_keypair.public_key.clone();
        let identity_pubkey = project_data.identity_keypair.public_key.clone();
        info!(
            "Project already exists: {:?} with pubkey: {:?} and identity: {:?}",
            project_data, signing_pubkey, identity_pubkey
        );

        if project_data.dapp_url != subscribe_topic_data.dapp_url {
            info!("Updating dapp_url for project: {}", project_id);
            db.collection::<ProjectData>("project_data")
                .update_one(
                    doc! { "_id": project_id.clone()},
                    doc! { "$set": { "dapp_url": &subscribe_topic_data.dapp_url } },
                    None,
                )
                .await?;
        }

        return Ok(Json(
            // TODO use struct
            json!({ "identityPublicKey": identity_pubkey, "subscribeTopicPublicKey": signing_pubkey}),
        )
        .into_response());
    };

    let mut rng = OsRng;

    let signing_secret = StaticSecret::from({
        let mut signing_secret: [u8; 32] = [0; 32];
        rng.fill_bytes(&mut signing_secret);
        signing_secret
    });
    let signing_public = PublicKey::from(&signing_secret);
    let topic = sha256::digest(signing_public.as_bytes());
    let signing_public = hex::encode(signing_public);

    let identity_secret = ed25519_dalek::SigningKey::generate(&mut rng);
    let identity_public = hex::encode(ed25519_dalek::VerifyingKey::from(&identity_secret));

    let project_data = ProjectData {
        id: project_id.clone(),
        signing_keypair: Keypair {
            private_key: hex::encode(signing_secret.to_bytes()),
            public_key: signing_public.clone(),
        },
        identity_keypair: Keypair {
            private_key: hex::encode(identity_secret.to_bytes()),
            public_key: identity_public.clone(),
        },
        dapp_url: subscribe_topic_data.dapp_url,
        topic: topic.clone(),
    };

    info!(
        "Saving project_info to database for project: {} with signing pubkey: {} and identity \
         pubkey: {}, topic: {}",
        project_id, signing_public, identity_public, topic
    );

    db.collection::<ProjectData>("project_data")
        .replace_one(
            doc! { "_id": project_id.clone()},
            project_data,
            ReplaceOptions::builder().upsert(true).build(),
        )
        .await?;

    info!("Subscribing to project topic: {}", &topic);

    state.wsclient.subscribe(topic.into()).await?;

    Ok(Json(
        // TODO use struct
        json!({ "identityPublicKey": identity_public, "subscribeTopicPublicKey": signing_public}),
    )
    .into_response())
}
