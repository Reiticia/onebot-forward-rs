use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Rule::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Rule::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Rule::ChatId).big_integer().not_null())
                    .col(
                        ColumnDef::new(Rule::ChatType)
                            .text()
                            .not_null()
                            .default("group"),
                    )
                    .col(
                        ColumnDef::new(Rule::ItemType)
                            .text()
                            .not_null()
                            .default("black_list"),
                    )
                    .col(
                        ColumnDef::new(Rule::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default("CURRENT_TIMESTAMP"),
                    )
                    .col(
                        ColumnDef::new(Rule::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default("CURRENT_TIMESTAMP"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Rule::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Rule {
    Table,
    Id,
    ChatId,
    ChatType,
    ItemType,
    CreatedAt,
    UpdatedAt,
}
