use {
    crate::{extractors::AuthedProjectId, state::AppState},
    axum::{self, extract::State, response::IntoResponse, Json},
    log::info,
    mongodb::{bson::doc, options::ReplaceOptions},
    rand::{rngs::StdRng, Rng},
    rand_core::SeedableRng,
    serde::{Deserialize, Serialize},
    serde_json::json,
    sha2::Digest,
    std::sync::Arc,
    x25519_dalek::{PublicKey, StaticSecret},
};

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectData {
    #[serde(rename = "_id")]
    pub id: String,
    pub private_key: String,
    pub public_key: String,
    pub topic: String,
}

pub async fn handler(
    State(state): State<Arc<AppState>>,
    AuthedProjectId(project_id, project_secret): AuthedProjectId,
) -> Result<axum::response::Response, crate::error::Error> {
    info!("Generating keypair for project: {}", project_id);
    let db = state.database.clone();

    let mut hasher = sha2::Sha256::new();
    hasher.update(project_secret.as_bytes());
    hasher.update(project_id.as_bytes());
    let seed = hasher.finalize();

    let mut rng: StdRng = SeedableRng::from_seed(seed.into());

    let secret = StaticSecret::from(rng.gen::<[u8; 32]>());
    let public = PublicKey::from(&secret);

    let public_key = hex::encode(public.as_bytes());

    let topic = sha256::digest(public.as_bytes());
    let project_data = ProjectData {
        id: project_id.clone(),
        private_key: hex::encode(secret.to_bytes()),
        public_key: public_key.clone(),
        topic: topic.clone(),
    };

    info!(
        "Saving project_info to database for project: {} with pubkey: {}",
        project_id, public_key
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

    Ok(Json(json!({ "publicKey": public_key })).into_response())
}
