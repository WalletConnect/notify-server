use {
    crate::{extractors::AuthedProjectId, state::AppState},
    axum::{self, extract::State, response::IntoResponse, Json},
    log::info,
    mongodb::{bson::doc, options::ReplaceOptions},
    rand::{rngs::StdRng, Rng},
    rand_core::SeedableRng,
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
    pub topic: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Keypair {
    pub private_key: String,
    pub public_key: String,
}

pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, _): AuthedProjectId,
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

        return Ok(Json(
            json!({ "identityPublicKey": identity_pubkey, "subscribeTopicPublicKey": signing_pubkey}),
        )
        .into_response());
    };

    let mut rng: StdRng = StdRng::from_entropy();

    let signing_secret = StaticSecret::from(rng.gen::<[u8; 32]>());
    let signing_public = PublicKey::from(&signing_secret);
    let topic = sha256::digest(signing_public.as_bytes());
    let signing_public = hex::encode(signing_public);

    let identity_secret = ed25519_dalek::SecretKey::generate(&mut rng);
    let identity_public = hex::encode(ed25519_dalek::PublicKey::from(&identity_secret));

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
        json!({ "identityPublicKey": identity_public, "subscribeTopicPublicKey": signing_public}),
    )
    .into_response())
}
