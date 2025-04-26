use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Rule::Table)
                    .add_column(ColumnDef::new(Rule::GroupChatId).integer().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Rule::Table)
                    .drop_column(Rule::GroupChatId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Rule {
    Table,
    GroupChatId,
}
