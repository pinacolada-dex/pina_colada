use cosmwasm_std::{Addr, CosmosMsg, StdResult, DepsMut, Deps};
use astroport::asset::{Asset, AssetInfo, DecimalAsset};
use astroport_pcl_common::state::Config;
use crate::error::ContractError;
use crate::state::{PAIR_BALANCES, Precisions};
// use std::str;
use itertools::Itertools;

pub(crate) fn query_pools(
    deps: &DepsMut,     
    config: &Config,
    precisions: &Precisions,
) -> Result<Vec<DecimalAsset>, ContractError> {
    let key = generate_key_from_asset_info([config.pair_info.asset_infos[0].clone(),config.pair_info.asset_infos[1].clone()].as_ref());
    println!("{}",key);
    let pairs = PAIR_BALANCES.load(deps.storage,key).unwrap();
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
    deps: Deps,     
    config: &Config,
    precisions: &Precisions,
) -> Result<Vec<DecimalAsset>, ContractError> {
    let key = generate_key_from_asset_info([config.pair_info.asset_infos[0].clone(),config.pair_info.asset_infos[1].clone()].as_ref());
    println!("{}",key);
    let pairs = PAIR_BALANCES.load(deps.storage,key).unwrap();
    println!("{:?}",pairs);
    pairs.into_iter()
        .map(|asset| {
            asset
                .to_decimal_asset(precisions.get_precision(&asset.info)?)
                .map_err(Into::into)
        })
        .collect()
}

pub fn get_transfer_messages(assets: &[Asset], recipient: &Addr) -> StdResult<Vec<CosmosMsg>> {
    assets
        .iter()
        .map(|asset| asset.clone().into_msg(recipient))  // Added clone()
        .collect()
}

pub fn get_rebalance_messages(
    remove_assets: &[Asset],
    add_assets: &[Asset],
    recipient: &Addr,
    config: &Config,
) -> StdResult<Vec<CosmosMsg>> {
    let mut messages = vec![];
    messages.extend(get_transfer_messages(remove_assets, recipient)?);
    messages.extend(get_transfer_messages(add_assets, &config.pair_info.contract_addr)?);
    Ok(messages)
}

pub fn update_pool_balances(
    deps: &mut DepsMut,
    pool_key: String,
    assets: &[Asset],
) -> StdResult<()> {
    let mut pair_balances = PAIR_BALANCES.load(deps.storage, pool_key.clone())?;
    for (i, asset) in assets.iter().enumerate() {
        pair_balances[i].amount = asset.amount;
    }
    PAIR_BALANCES.save(deps.storage, pool_key, &pair_balances)
}

pub fn generate_key_from_asset_info(assets: &[AssetInfo]) -> String {
    std::str::from_utf8(&pair_key(&[assets[0].clone(), assets[1].clone()]))  // Added std::
        .unwrap()
        .to_string()
}

pub fn pair_key(asset_infos: &[AssetInfo]) -> Vec<u8> {
    asset_infos
        .iter()
        .map(AssetInfo::as_bytes)
        .sorted()
        .flatten()
        .copied()
        .collect()
}