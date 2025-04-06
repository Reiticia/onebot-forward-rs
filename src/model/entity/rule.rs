use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "rule")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub chat_id: i64,
    pub chat_type: ChatType,
    pub item_type: ItemType,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum ChatType {
    #[sea_orm(string_value = "group")]
    Group,
    #[sea_orm(string_value = "private")]
    Private,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum ItemType {
    #[sea_orm(string_value = "black_list")]
    BlackList,
    #[sea_orm(string_value = "white_list")]
    WhiteList,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
