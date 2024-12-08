use std::convert::TryFrom;

use astroport::asset::Asset;
use astroport::cosmwasm_ext::{DecimalToInteger, IntegerToDecimal};
use astroport::pair::ConfigResponse;
use astroport::pair::SimulationResponse;
use astroport::querier::query_supply;
use astroport::router::{SimulateSwapOperationsResponse};
use astroport_pcl_common::{calc_d, get_xcp};
use astroport_pcl_common::utils::compute_swap;
use crate::state::Precisions;
use astroport_pcl_common::utils::before_swap_check;
use cosmwasm_std::{to_json_binary, Addr, Decimal, Decimal256, Deps, Env, StdError, Uint128};
use itertools::Itertools;
use astroport::pair_concentrated::ConcentratedPoolConfig;
use crate::error::ContractError;
use crate::handlers::{generate_key_from_asset_info, LP_TOKEN_PRECISION};
use crate::msg::SwapOperation;
use crate::state::{ POOLS};
use crate::utils::{query_pools_sim};
pub fn simulate_swap_operations(
    deps: Deps,
    env:Env,
    offer_amount: Uint128,
    operations: Vec<SwapOperation>,
) -> Result<SimulateSwapOperationsResponse, ContractError> {
    //assert_operations(deps.api, &operations)?;


    let mut return_amount = offer_amount;

    for operation in operations.into_iter() {
        let (offer_asset_info,ask_asset_info)= (operation.offer_asset_info,operation.ask_asset_info);
        let pool_key=generate_key_from_asset_info(&[offer_asset_info.clone(),ask_asset_info.clone()].to_vec());
        let offer_asset=  Asset {
            info: offer_asset_info.clone(),
            amount:return_amount,
        };
        let subresult=query_simulation(deps,env.clone(),offer_asset,pool_key).unwrap();
        return_amount=subresult.return_amount;
    }

    Ok(SimulateSwapOperationsResponse {
        amount: return_amount,
    })
}

pub fn query_simulation(
    deps: Deps,
    env: Env,
    offer_asset: Asset,
    pool_key:String
) -> Result<SimulationResponse, ContractError> {
    let config = POOLS.load(deps.storage,pool_key.clone())?;
    let precisions = Precisions::new(deps.storage)?;
    let offer_asset_prec = precisions.get_precision(&offer_asset.info)?;
    let offer_asset_dec = offer_asset.to_decimal_asset(offer_asset_prec)?;

    let pools = query_pools_sim(deps, &config, &precisions)?;

    let (offer_ind, _) = pools
        .iter()
        .find_position(|asset| asset.info == offer_asset.info)
        .ok_or_else(|| ContractError::InvalidAsset(offer_asset_dec.info.to_string()))?;
    let ask_ind = 1 - offer_ind;
    let ask_asset_prec = precisions.get_precision(&pools[ask_ind].info)?;

    before_swap_check(&pools, offer_asset_dec.amount)?;

    let xs = pools.iter().map(|asset| asset.amount).collect_vec();

   
    let maker_fee_share = Decimal256::zero();
    
    // If this pool is configured to share fees
    let share_fee_share = Decimal256::zero();
   
    let swap_result = compute_swap(
        &xs,
        offer_asset_dec.amount,
        ask_ind,
        &config,
        &env,
        maker_fee_share,
        share_fee_share,
    )?;

    Ok(SimulationResponse {
        return_amount: swap_result.dy.to_uint(ask_asset_prec)?,
        spread_amount: swap_result.spread_fee.to_uint(ask_asset_prec)?,
        commission_amount: swap_result.total_fee.to_uint(ask_asset_prec)?,
    })
}
/// Compute the current LP token virtual price.
pub fn query_lp_price(deps: Deps, env: Env, pool_key:String) -> Result<Decimal256,ContractError> {
    let config = POOLS.load(deps.storage,pool_key.clone())?;
    let total_lp = query_supply(&deps.querier, &config.pair_info.liquidity_token)?
        .to_decimal256(LP_TOKEN_PRECISION)?;
    if !total_lp.is_zero() {
        let precisions = Precisions::new(deps.storage)?;
        let mut ixs = query_pools_sim(deps, &config, &precisions)
            .map_err(|err| ContractError::Std(StdError::generic_err(err.to_string())))?
            .into_iter()
            .map(|asset| asset.amount)
            .collect_vec();
        ixs[1] *= config.pool_state.price_state.price_scale;
        let amp_gamma = config.pool_state.get_amp_gamma(&env);
        let d = calc_d(&ixs, &amp_gamma)?;
        let xcp = get_xcp(d, config.pool_state.price_state.price_scale);

        Ok(xcp / total_lp)
    } else {
        Ok(Decimal256::zero())
    }
}

/// Returns the pair contract configuration.
pub fn query_config(deps: Deps, env: Env,pool_key:String) -> Result<ConfigResponse,ContractError> {
    let config = POOLS.load(deps.storage,pool_key)?;
    let amp_gamma = config.pool_state.get_amp_gamma(&env);
    let dec256_price_scale = config.pool_state.price_state.price_scale;
    let price_scale = Decimal::from_atomics(
        Uint128::try_from(dec256_price_scale.atomics())?,
        dec256_price_scale.decimal_places(),
    )
    .map_err(|e| StdError::generic_err(format!("{e}")))?;

   
    Ok(ConfigResponse {
        block_time_last: 0, // keeping this field for backwards compatibility
        params: Some(to_json_binary(&ConcentratedPoolConfig {
            amp: amp_gamma.amp,
            gamma: amp_gamma.gamma,
            mid_fee: config.pool_params.mid_fee,
            out_fee: config.pool_params.out_fee,
            fee_gamma: config.pool_params.fee_gamma,
            repeg_profit_threshold: config.pool_params.repeg_profit_threshold,
            min_price_scale_delta: config.pool_params.min_price_scale_delta,
            price_scale,
            ma_half_time: config.pool_params.ma_half_time,
            track_asset_balances: config.track_asset_balances,
            fee_share: config.fee_share,
        })?),
        owner: config.owner.unwrap_or(Addr::unchecked("Pina Colada")),
        factory_addr: Addr::unchecked("Pina Colada"),
    })
}

/// Compute the current pool D value.
pub fn query_compute_d(deps: Deps, env: Env,pool_key:String) -> Result<Decimal256,ContractError> {
    let config = POOLS.load(deps.storage,pool_key)?;
    let precisions = Precisions::new(deps.storage)?;

    let mut xs= query_pools_sim(deps, &config, &precisions)
        .map_err(|e| StdError::generic_err(e.to_string()))?
        .into_iter()
        .map(|a| a.amount)
        .collect_vec();

    if xs[0].is_zero() || xs[1].is_zero() {
        return Err(ContractError::InvalidZeroAmount{});
    }

    xs[1] *= config.pool_state.price_state.price_scale;

    let amp_gamma = config.pool_state.get_amp_gamma(&env);
    Ok(calc_d(&xs, &amp_gamma)?)
}
