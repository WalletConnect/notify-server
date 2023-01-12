use {
    crate::stores::{self, StoreError::NotFound},
    async_trait::async_trait,
    sqlx::Executor,
    std::sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct TableRow {
    pub name: String,
}

pub type ExampleStoreArc = Arc<dyn ExampleStore + Send + Sync + 'static>;

#[async_trait]
pub trait ExampleStore {
    async fn create_row(&self, id: &str, row: TableRow) -> stores::Result<()>;
    async fn get_row(&self, id: &str) -> stores::Result<TableRow>;
    async fn delete_row(&self, id: &str) -> stores::Result<()>;
}

#[async_trait]
impl ExampleStore for sqlx::PgPool {
    async fn create_row(&self, id: &str, row: TableRow) -> stores::Result<()> {
        let mut query_builder =
            sqlx::QueryBuilder::new("INSERT INTO public.example_table (id, name) ");
        query_builder.push_values(vec![(id, row.name)], |mut b, row| {
            b.push_bind(row.0).push_bind(row.1);
        });
        let query = query_builder.build();

        self.execute(query).await?;

        Ok(())
    }

    async fn get_row(&self, id: &str) -> stores::Result<TableRow> {
        let res = sqlx::query_as::<sqlx::postgres::Postgres, TableRow>(
            "SELECT name FROM public.example_table WHERE id = $1",
        )
        .bind(id)
        .fetch_one(self)
        .await;

        match res {
            Err(sqlx::Error::RowNotFound) => Err(NotFound("example".to_string(), id.to_string())),
            Err(e) => Err(e.into()),
            Ok(row) => Ok(row),
        }
    }

    async fn delete_row(&self, id: &str) -> stores::Result<()> {
        let mut query_builder =
            sqlx::QueryBuilder::new("DELETE FROM public.example_table WHERE id = ");
        query_builder.push_bind(id);
        let query = query_builder.build();

        match self.execute(query).await {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
