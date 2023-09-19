use {
    crate::Result,
    mongodb::bson::doc,
    serde::{Deserialize, Serialize},
    sqlx::Postgres,
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

pub async fn migrate(mongo: &mongodb::Database, postgres: &sqlx::PgPool) -> Result<()> {
    let mut projects_cursor = mongo
        .collection::<ProjectData>("project_data")
        .find(None, None)
        .await?;

    while projects_cursor.advance().await? {
        let project = projects_cursor.deserialize_current()?;

        sqlx::query::<Postgres>(
            "
            INSERT INTO projects (
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

    // TODO startup code to migrate subscribers and webhooks

    Ok(())
}
