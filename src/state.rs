use astroport::asset::{Asset, AssetInfo};
use cosmwasm_std::{CustomQuery, Order, StdResult, Storage, Uint128};
use cw20::{Cw20QueryMsg, TokenInfoResponse};
use cw_storage_plus::{Item, Map, SnapshotMap};
use itertools::Itertools;

use astroport_pcl_common::{error::PclError, state::Config};
use cosmwasm_std::DepsMut;
/// Stores pool parameters and state.

pub struct Precisions(Vec<(String, u8)>);

impl<'a> Precisions {
    /// Stores map of AssetInfo (as String) -> precision
    const PRECISIONS: Map<'a, String, u8> = Map::new("precisions");
    pub fn new(storage: &dyn Storage) -> StdResult<Self> {
        let items = Self::PRECISIONS
            .range(storage, None, None, Order::Ascending)
            .collect::<StdResult<Vec<_>>>()?;

        Ok(Self(items))
    }

    /// Store all token precisions
    pub fn store_precisions<C: CustomQuery>(
        deps: DepsMut<C>,
        asset_infos: &[AssetInfo],       
    ) -> StdResult<()> {
        for asset_info in asset_infos {
            let decimals= match asset_info {
                AssetInfo::NativeToken { denom: _ } => {
                    18u8
                    
                }
                AssetInfo::Token { contract_addr } => {
                    let res: TokenInfoResponse =
                        deps.querier.query_wasm_smart(contract_addr, &Cw20QueryMsg::TokenInfo {})?;
        
                    res.decimals
                }
            };
           
       
            Self::PRECISIONS.save(deps.storage, asset_info.to_string(),  &decimals)?;
        }

        Ok(())
    }

    pub fn get_precision(&self, asset_info: &AssetInfo) -> Result<u8, PclError> {
        self.0
            .iter()
            .find_map(|(info, prec)| {
                if info == &asset_info.to_string() {
                    Some(*prec)
                } else {
                    None
                }
            })
            .ok_or_else(|| PclError::InvalidAsset(asset_info.to_string()))
    }
}

pub const QUEUED_MINT: Item<String> = Item::new("pool_key");
pub const POOLS: Map<String, Config> = Map::new("pools");
pub const PAIR_BALANCES: Map<String, Vec<Asset>> = Map::new("pair_balances");
/// Stores asset balances to query them later at any block height
pub const BALANCES: SnapshotMap<&AssetInfo, Uint128> = SnapshotMap::new(
    "balances",
    "balances_check",
    "balances_change",
    cw_storage_plus::Strategy::EveryBlock,
);
pub fn find_asset_index(deps: &mut DepsMut, key: String, asset: Asset) -> usize {
    let balances = PAIR_BALANCES.load(deps.storage, key.clone()).unwrap();

    balances
        .iter()
        .enumerate()
        .find(|&r| r.1.info == asset.info)
        .unwrap()
        .0
}

pub fn increment_asset_balance(deps: &mut DepsMut, key: String, index: usize, amount: Uint128) {
    let mut balances = PAIR_BALANCES.load(deps.storage, key.clone()).unwrap();

    balances[index].amount += amount;
    let _ = PAIR_BALANCES.save(deps.storage, key, &balances);
}
pub fn decrease_asset_balance(deps: &mut DepsMut, key: String, index: usize, amount: Uint128) {
    let mut balances = PAIR_BALANCES.load(deps.storage, key.clone()).unwrap();

    balances[index].amount -= amount;
    let _ = PAIR_BALANCES.save(deps.storage, key, &balances);
}
pub fn increment_pair_balances(deps: &mut DepsMut, key: String, amounts: Vec<Uint128>) {
    let mut curr = PAIR_BALANCES.load(deps.storage, key.clone()).unwrap();
    for (i, v) in amounts.into_iter().enumerate() {
        curr[i].amount += v;
    }
    let _ = PAIR_BALANCES.save(deps.storage, key, &curr);
}

pub fn decrease_pair_balances(deps: &mut DepsMut, key: String, amounts: Vec<Uint128>) {
    let mut curr = PAIR_BALANCES.load(deps.storage, key.clone()).unwrap();
    for (i, v) in amounts.into_iter().enumerate() {
        println!("{} {} amounts", curr[i], v);
        curr[i].amount -= v;
    }
    let _ = PAIR_BALANCES.save(deps.storage, key, &curr);
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
