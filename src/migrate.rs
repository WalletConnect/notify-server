use {
    crate::{
        model::helpers::{get_project_by_project_id, upsert_project, upsert_subscriber},
        websocket_service::decode_key,
        Result,
    },
    ed25519_dalek::SigningKey,
    mongodb::bson::doc,
    serde::{Deserialize, Serialize},
    std::collections::HashSet,
    x25519_dalek::{PublicKey, StaticSecret},
};

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectData {
    #[serde(rename = "_id")]
    pub id: String,
    pub identity_keypair: Keypair,
    pub signing_keypair: Keypair,
    pub app_domain: String,
    pub topic: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Keypair {
    pub private_key: String,
    pub public_key: String,
}

impl Keypair {
    pub fn from_subscribe_key(key: &StaticSecret) -> Self {
        Self {
            public_key: hex::encode(PublicKey::from(key)),
            private_key: hex::encode(key),
        }
    }

    pub fn from_authentication_key(value: &SigningKey) -> Self {
        Self {
            public_key: hex::encode(value.verifying_key()),
            private_key: hex::encode(value.to_bytes()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    #[serde(rename = "_id")]
    pub id: String,
    pub relay_url: String,
    pub sym_key: String,
    pub expiry: u64,
    pub scope: HashSet<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LookupEntry {
    #[serde(rename = "_id")]
    pub topic: String,
    pub project_id: String,
    pub account: String,
    pub expiry: u64,
}

fn decode_authentication_private_key(authentication_private_key: &str) -> Result<SigningKey> {
    Ok(SigningKey::from_bytes(&decode_key(
        authentication_private_key,
    )?))
}

fn decode_subscribe_private_key(subscribe_key: &str) -> Result<StaticSecret> {
    Ok(StaticSecret::from(decode_key(subscribe_key)?))
}

pub async fn migrate(mongo: &mongodb::Database, postgres: &sqlx::PgPool) -> Result<()> {
    let mut projects_cursor = mongo
        .collection::<ProjectData>("project_data")
        .find(None, None)
        .await?;

    while projects_cursor.advance().await? {
        let project = projects_cursor.deserialize_current()?;

        upsert_project(
            project.id.clone().into(),
            &project.app_domain,
            project.topic.into(),
            &decode_authentication_private_key(&project.identity_keypair.private_key)?,
            &decode_subscribe_private_key(&project.signing_keypair.private_key)?,
            postgres,
        )
        .await?;

        mongo
            .collection::<ProjectData>("project_data")
            .delete_one(doc! {"_id": project.id}, None)
            .await?;
    }

    let mut lookup_entry_cursor = mongo
        .collection::<LookupEntry>("lookup_table")
        .find(None, None)
        .await?;

    while lookup_entry_cursor.advance().await? {
        let lookup_entry = lookup_entry_cursor.deserialize_current()?;
        if let Some(client_data) = mongo
            .collection::<ClientData>(&lookup_entry.project_id)
            .find_one(doc! {"_id": lookup_entry.account.clone()}, None)
            .await?
        {
            match get_project_by_project_id(lookup_entry.project_id.clone().into(), postgres).await
            {
                Ok(project) => {
                    let notify_key = hex::decode(&client_data.sym_key)?.try_into().unwrap();
                    upsert_subscriber(
                        project.id,
                        client_data.id.into(),
                        client_data.scope,
                        &notify_key,
                        sha256::digest(&notify_key).into(),
                        postgres,
                    )
                    .await?;
                }
                Err(sqlx::Error::RowNotFound) => {
                    // no-op
                }
                Err(e) => {
                    return Err(e.into());
                }
            }

            mongo
                .collection::<ClientData>(&lookup_entry.project_id)
                .delete_one(doc! {"_id": lookup_entry.account}, None)
                .await?;
        }

        mongo
            .collection::<LookupEntry>("lookup_table")
            .delete_one(doc! {"_id": lookup_entry.topic}, None)
            .await?;
    }

    Ok(())
}
