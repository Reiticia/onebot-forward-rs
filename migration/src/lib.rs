pub use sea_orm_migration::prelude::*;

mod m20250406_000001_create_table;
mod m20250426_000002_add_group_chat_id_column;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250406_000001_create_table::Migration),
            Box::new(m20250426_000002_add_group_chat_id_column::Migration),
        ]
    }
}
