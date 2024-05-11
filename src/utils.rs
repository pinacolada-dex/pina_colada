use cosmwasm_std::{DepsMut,Deps};
use crate::state::Precisions;
use astroport::asset::DecimalAsset;
use crate::state::PAIR_BALANCES;
use crate::error::ContractError;
use astroport_pcl_common::state::Config;
use crate::handlers::generate_key_from_asset_info;
pub(crate) fn query_pools(
    deps:&DepsMut,      
    config: &Config,
    precisions: &Precisions,
) -> Result<Vec<DecimalAsset>, ContractError> {
    //
    let key=generate_key_from_asset_info(&([config.pair_info.asset_infos[0].clone(),config.pair_info.asset_infos[1].clone()].to_vec()));
    println!("{}",key);
    let pairs=PAIR_BALANCES.load(deps.storage,key).unwrap();
    println!("{:?}",pairs);
    pairs.into_iter()
    .map(|asset| {
        asset
            .to_decimal_asset(precisions.get_precision(&asset.info)?)
            .map_err(Into::into)
    })
    .collect()
    
}

pub(crate) fn query_pools_sim(
    deps:Deps,      
    config: &Config,
    precisions: &Precisions,
) -> Result<Vec<DecimalAsset>, ContractError> {
    //
    let key=generate_key_from_asset_info(&([config.pair_info.asset_infos[0].clone(),config.pair_info.asset_infos[1].clone()].to_vec()));
    println!("{}",key);
    let pairs=PAIR_BALANCES.load(deps.storage,key).unwrap();
    println!("{:?}",pairs);
    pairs.into_iter()
    .map(|asset| {
        asset
            .to_decimal_asset(precisions.get_precision(&asset.info)?)
            .map_err(Into::into)
    })
    .collect()
    
}
