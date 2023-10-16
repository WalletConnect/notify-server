use {
    crate::{model::helpers::get_project_by_project_id, Result},
    chrono::{Duration, Utc},
    mongodb::bson::doc,
    serde::{Deserialize, Serialize},
    sqlx::Postgres,
    std::collections::HashSet,
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

pub async fn migrate(mongo: &mongodb::Database, postgres: &sqlx::PgPool) -> Result<()> {
    let mut projects_cursor = mongo
        .collection::<ProjectData>("project_data")
        .find(None, None)
        .await?;

    while projects_cursor.advance().await? {
        let project = projects_cursor.deserialize_current()?;

        sqlx::query::<Postgres>(
            "
            INSERT INTO project (
                project_id,
                app_domain,
                topic,
                authentication_public_key,
                authentication_private_key,
                subscribe_public_key,
                subscribe_private_key
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
        ",
        )
        .bind(project.id.clone())
        .bind(project.app_domain)
        .bind(project.topic)
        .bind(project.identity_keypair.public_key)
        .bind(project.identity_keypair.private_key)
        .bind(project.signing_keypair.public_key)
        .bind(project.signing_keypair.private_key)
        .execute(postgres)
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
        let client_data = mongo
            .collection::<ClientData>(&lookup_entry.project_id)
            .find_one(doc! {"_id": lookup_entry.account.clone()}, None)
            .await?
            .unwrap();

        let project =
            get_project_by_project_id(lookup_entry.project_id.clone().into(), postgres).await?;

        sqlx::query::<Postgres>(
            "
            INSERT INTO SUBSCRIBER (
                project,
                account,
                sym_key,
                topic,
                expiry
            ) VALUES ($1, $2, $3, $4, $5)
            ",
        )
        .bind(project.id)
        .bind(client_data.id)
        .bind(client_data.sym_key)
        .bind(lookup_entry.topic.clone())
        .bind(Utc::now() + Duration::days(30))
        .execute(postgres)
        .await?;

        // TODO migrate scopes

        mongo
            .collection::<LookupEntry>("lookup_table")
            .delete_one(doc! {"_id": lookup_entry.topic}, None)
            .await?;
        mongo
            .collection::<ClientData>(&lookup_entry.project_id)
            .delete_one(doc! {"_id": lookup_entry.account}, None)
            .await?;
    }

    Ok(())
}
