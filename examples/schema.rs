use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use astroport::router::{InstantiateMsg, MigrateMsg};
use astrpt::msg::{ExecuteMsg, QueryMsg};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Schema {
    instantiate: InstantiateMsg,
    execute: ExecuteMsg,
    query: QueryMsg,
    migrate: MigrateMsg,
}

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema(&schema_for!(Schema), &out_dir);
}
