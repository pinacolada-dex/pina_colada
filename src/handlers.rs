use astroport::asset::{
    addr_opt_validate, format_lp_token_name, Asset, AssetInfo, CoinsExt, Decimal256Ext, PairInfo,
    MINIMUM_LIQUIDITY_AMOUNT,
};
use astroport::cosmwasm_ext::{AbsDiff as _, DecimalToInteger, IntegerToDecimal};
use astroport::factory::PairType;
use astroport::observation::PrecommitObservation;
use astroport::pair::MIN_TRADE_SIZE;
use astroport::querier::query_supply;
use astroport::token::InstantiateMsg as TokenInstantiateMsg;

use astroport::pair_concentrated::{ConcentratedPoolParams, UpdatePoolParams};

use astroport_pcl_common::state::{
    AmpGamma, Config, PoolParams, PoolState,  PriceState,
};
use astroport_pcl_common::utils::{
    assert_max_spread, assert_slippage_tolerance, before_swap_check, calc_provide_fee,
    check_asset_infos, check_assets, compute_swap, get_share_in_assets,
    mint_liquidity_token_message,
};
use astroport_pcl_common::{calc_d, get_xcp};
use cosmwasm_schema::serde::de;
use std::str;

use crate::error::ContractError;
use crate::msg::SwapOperation;
use crate::state::{
    decrease_asset_balance, decrease_pair_balances, find_asset_index, increment_asset_balance,
    increment_pair_balances, pair_key, BALANCES, PAIR_BALANCES, POOLS, QUEUED_MINT,Precisions
};
use crate::utils::query_pools;
use cosmwasm_std::{
    attr, from_binary, to_binary, wasm_execute, wasm_instantiate, Addr, Api, BankMsg, Binary, Coin,
    CosmosMsg, Decimal, Decimal256, DepsMut, Env, MessageInfo, Response, StdError, StdResult,
    SubMsg, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, MinterResponse};
use itertools::Itertools;
pub(crate) const LP_TOKEN_PRECISION: u8 = 6;
const MAX_SWAP_OPERATIONS: usize = 10;
const DUMMY_ADDRESS: &str = "PINA_COLADA";
const INSTANTIATE_TOKEN_REPLY_ID: u64 = 1;
/// Returns the end result of a simulation for one or multiple swap
/// operations using a [`SimulateSwapOperationsResponse`] object.
///
/// * **offer_amount** amount of offer assets being swapped.
///
/// * **operations** is a vector that contains objects of type [`SwapOperation`].
/// These are all the swap operations for which we perform a simulation.
pub static DENOM: &str = "aarch";

pub fn generate_key_from_assets(assets: &Vec<Asset>) -> String {
    str::from_utf8(&pair_key(&[assets[0].clone().info, assets[1].clone().info]))
        .unwrap()
        .to_string()
}
pub fn generate_key_from_asset_info(assets: &Vec<AssetInfo>) -> String {
    str::from_utf8(&pair_key(&[assets[0].clone(), assets[1].clone()]))
        .unwrap()
        .to_string()
}
pub fn send_native(to: &Addr, amount: Uint128) -> StdResult<CosmosMsg> {
    let msg = BankMsg::Send {
        to_address: to.into(),
        amount: ([Coin {
            denom: String::from(DENOM),
            amount,
        }])
        .to_vec(),
    };
    Ok(msg.into())
}
/// Validates swap operations.
///
/// * **operations** is a vector that contains objects of type [`SwapOperation`]. These are all the swap operations we check.
fn assert_operations(api: &dyn Api, operations: &[SwapOperation]) -> Result<(), ContractError> {
    let operations_len = operations.len();
    if operations_len == 0 {
        return Err(ContractError::MustProvideOperations {});
    }

    if operations_len > MAX_SWAP_OPERATIONS {
        return Err(ContractError::SwapLimitExceeded {});
    }

    let mut prev_ask_asset: Option<AssetInfo> = None;

    for operation in operations {
        let (offer_asset, ask_asset) = (
            operation.offer_asset_info.clone(),
            operation.ask_asset_info.clone(),
        );

        offer_asset.check(api)?;
        ask_asset.check(api)?;

        if offer_asset.equal(&ask_asset) {
            return Err(ContractError::DoublingAssetsPath {
                offer_asset: offer_asset.to_string(),
                ask_asset: ask_asset.to_string(),
            });
        }

        if let Some(prev_ask_asset) = prev_ask_asset {
            if prev_ask_asset != offer_asset {
                return Err(ContractError::InvalidPathOperations {
                    prev_ask_asset: prev_ask_asset.to_string(),
                    next_offer_asset: offer_asset.to_string(),
                    next_ask_asset: ask_asset.to_string(),
                });
            }
        }

        prev_ask_asset = Some(ask_asset);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn execute_provide_liquidity(
    deps: &mut DepsMut,
    env: Env,
    info: MessageInfo,
    mut assets: Vec<Asset>,
    slippage_tolerance: Option<Decimal>,
    auto_stake: Option<bool>,
    receiver: Option<String>,
) -> Result<Response, ContractError> {
    let pool_key = generate_key_from_assets(&assets);

    let mut config = POOLS.load(deps.storage, pool_key.clone())?;
    //println!("{:?} {}", config, String::from("CONFIG HERE "));
    //println!("{:?}", assets.len());

    match assets.len() {
        0 => {
            return Err(StdError::generic_err("Nothing to provide").into());
        }
        1 => {
            // Append omitted asset with explicit zero amount
            let (given_ind, _) = config
                .pair_info
                .asset_infos
                .iter()
                .find_position(|pool| pool.equal(&assets[0].info))
                .ok_or_else(|| ContractError::InvalidAsset(assets[0].info.to_string()))?;
            assets.push(Asset {
                info: config.pair_info.asset_infos[1 ^ given_ind].clone(),
                amount: Uint128::zero(),
            });
        }
        2 => {}
        _ => {
            return Err(ContractError::InvalidNumberOfAssets(
                config.pair_info.asset_infos.len(),
            ))
        }
    }
    // get assets indices
    let first_asset_index = find_asset_index(deps, pool_key.clone(), assets[0].clone());
    let second_asset_index = 1 ^ first_asset_index;

    //println!("CHECKING ASSETS");
    check_assets(deps.api, &assets)?;
    //println!("CHECKING SENT");
    info.funds
        .assert_coins_properly_sent(&assets, &config.pair_info.asset_infos)?;

    let precisions = Precisions::new(deps.storage)?;

    //println!("QUERY POOLS");
    let mut pools = query_pools(&deps, &config, &precisions)?;

    if pools[0].info.equal(&assets[1].info) {
        assets.swap(0, 1);
    }

    let deposits = [
        Decimal256::with_precision(assets[0].amount, precisions.get_precision(&assets[0].info)?)?,
        Decimal256::with_precision(assets[1].amount, precisions.get_precision(&assets[1].info)?)?,
    ];

    //println!("QUERY SHARE");
    //println!("{}", &config.pair_info.liquidity_token);
    let total_share = query_supply(&deps.querier, &config.pair_info.liquidity_token)?
        .to_decimal256(LP_TOKEN_PRECISION)?;
    //println!("{}", total_share);
    // Initial provide can not be one-sided
    if total_share.is_zero() && (deposits[0].is_zero() || deposits[1].is_zero()) {
        return Err(ContractError::InvalidZeroAmount {});
    }
    //println!("TRANSFERRING TOKENS");
    increment_pair_balances(
        deps,
        pool_key.clone(),
        [assets[0].amount, assets[1].amount].to_vec(),
    );

    let mut messages = vec![];
    for (i, pool) in pools.iter_mut().enumerate() {
        //println!("{} {}", pool.amount, "the current pool amount");
        // If the asset is a token contract, then we need to execute a TransferFrom msg to receive assets
        match &pool.info {
            AssetInfo::Token { contract_addr } => {
                if !deposits[i].is_zero() {
                    messages.push(CosmosMsg::Wasm(wasm_execute(
                        contract_addr,
                        &Cw20ExecuteMsg::TransferFrom {
                            owner: info.sender.to_string(),
                            recipient: env.contract.address.to_string(),
                            amount: deposits[i]
                                .to_uint(precisions.get_precision(&assets[i].info)?)?,
                        },
                        vec![],
                    )?))
                }
            }
            AssetInfo::NativeToken { .. } => {

                // If the asset is native token, the pool balance is already increased
                // To calculate the total amount of deposits properly, we should subtract the user deposit from the pool
                //pool.amount = pool.amount.checked_sub(deposits[i])?;
            }
        }
    }

    let mut new_xp = pools
        .iter()
        .enumerate()
        .map(|(ind, pool)| pool.amount + deposits[ind])
        .collect_vec();
    //println!("{:?}", new_xp);
    new_xp[1] *= config.pool_state.price_state.price_scale;
    //println!("{:?}", new_xp);
    let amp_gamma = config.pool_state.get_amp_gamma(&env);
    let new_d = calc_d(&new_xp, &amp_gamma)?;

    let share = if total_share.is_zero() {
        //println!("total share is zero");
        let xcp = get_xcp(new_d, config.pool_state.price_state.price_scale);
        //println!("{:?}", xcp);
        let mint_amount = xcp
            .checked_sub(MINIMUM_LIQUIDITY_AMOUNT.to_decimal256(LP_TOKEN_PRECISION)?)
            .map_err(|_| ContractError::MinimumLiquidityAmountError {})?;
        //println!("{:?}", mint_amount);
        messages.extend(mint_liquidity_token_message(
            deps.querier,
            &config,
            &env.contract.address,
            &env.contract.address,
            MINIMUM_LIQUIDITY_AMOUNT,
            false,
        )?);

        // share cannot become zero after minimum liquidity subtraction
        if mint_amount.is_zero() {
            return Err(ContractError::MinimumLiquidityAmountError {});
        }

        config.pool_state.price_state.xcp_profit_real = Decimal256::one();
        config.pool_state.price_state.xcp_profit = Decimal256::one();

        mint_amount
    } else {
        //println!("total share note zero");
        //println!("{:?}", pools);
        let mut old_xp = pools.iter().map(|a| a.amount).collect_vec();

        old_xp[1] *= config.pool_state.price_state.price_scale;
        //println!("{:?}", old_xp);
        let old_d = calc_d(&old_xp, &amp_gamma)?;
        let share = (total_share * new_d / old_d).saturating_sub(total_share);

        let mut ideposits = deposits;
        ideposits[1] *= config.pool_state.price_state.price_scale;

        share * (Decimal256::one() - calc_provide_fee(&ideposits, &new_xp, &config.pool_params))
    };

    // calculate accrued share
    let share_ratio = share / (total_share + share);
    //println!("share ratio");
    //println!("{:?}", share_ratio);
    let balanced_share = vec![
        new_xp[0] * share_ratio,
        new_xp[1] * share_ratio / config.pool_state.price_state.price_scale,
    ];

    let assets_diff = vec![
        deposits[0].diff(balanced_share[0]),
        deposits[1].diff(balanced_share[1]),
    ];

    let mut slippage = Decimal256::zero();

    //println!("asset difference");
    //println!("{:?}", balanced_share);

    //println!("{:?}", assets_diff);
    // If deposit doesn't diverge too much from the balanced share, we don't update the price
    if assets_diff[0] >= MIN_TRADE_SIZE && assets_diff[1] >= MIN_TRADE_SIZE {
        //println!("UPDATING PRICE");
        slippage = assert_slippage_tolerance(
            &deposits,
            share,
            &config.pool_state.price_state,
            slippage_tolerance,
        )?;

        let last_price = assets_diff[0] / assets_diff[1];
        config.pool_state.update_price(
            &config.pool_params,
            &env,
            total_share + share,
            &new_xp,
            last_price,
        )?;
    }

    let share_uint128 = share.to_uint(LP_TOKEN_PRECISION)?;
    ////println!("UPDATING PRICE");
    // Mint LP tokens for the sender or for the receiver (if set)
    let receiver = addr_opt_validate(deps.api, &receiver)?.unwrap_or_else(|| info.sender.clone());
    let auto_stake = auto_stake.unwrap_or(false);
    messages.extend(mint_liquidity_token_message(
        deps.querier,
        &config,
        &env.contract.address,
        &receiver,
        share_uint128,
        auto_stake,
    )?);

    if config.track_asset_balances {
        for (i, pool) in pools.iter().enumerate() {
            BALANCES.save(
                deps.storage,
                &pool.info,
                &pool
                    .amount
                    .checked_add(deposits[i])?
                    .to_uint(precisions.get_precision(&pool.info)?)?,
                env.block.height,
            )?;
        }
    }

    POOLS.save(deps.storage, pool_key, &config)?;

    let attrs = vec![
        attr("action", "provide_liquidity"),
        attr("sender", info.sender),
        attr("receiver", receiver),
        attr("assets", format!("{}, {}", &assets[0], &assets[1])),
        attr("share", share_uint128),
        attr("slippage", slippage.to_string()),
    ];

    Ok(Response::new().add_messages(messages).add_attributes(attrs))
}
pub fn execute_withdraw_liquidity(
    deps: &mut DepsMut,
    env: Env,
    info: MessageInfo,
    sender: Addr,
    amount: Uint128,
    assets: Vec<Asset>,
) -> Result<Response, ContractError> {
    let pool = generate_key_from_assets(&assets);
    let mut config = POOLS.load(deps.storage, pool.clone())?;
   ;
    if info.sender != config.pair_info.liquidity_token {
        return Err(ContractError::Unauthorized {});
    }

    let precisions = Precisions::new(deps.storage)?;
    let pools = query_pools(&deps, &config, &precisions)?;

    decrease_pair_balances(deps, pool.clone(), [amount, amount].to_vec());

    let total_share = query_supply(&deps.querier, &config.pair_info.liquidity_token)?;
    let mut messages = vec![];

    let refund_assets =
        get_share_in_assets(&pools, amount.saturating_sub(Uint128::one()), total_share);
    // Commented this out
    // Not sure sure about the meaning of imbalanced withdraw
    /*  let refund_assets = if assets.is_empty() {
        // Usual withdraw (balanced)
        get_share_in_assets(&pools, amount.saturating_sub(Uint128::one()), total_share)
    } else {
        return Err(StdError::generic_err("Imbalanced withdraw is currently disabled").into());
    };
    */
    //println!("CP");
    // decrease XCP
    let mut xs = pools.iter().map(|a| a.amount).collect_vec();

    xs[0] -= refund_assets[0].amount;
    xs[1] -= refund_assets[1].amount;
    xs[1] *= config.pool_state.price_state.price_scale;
    let amp_gamma = config.pool_state.get_amp_gamma(&env);
    let d = calc_d(&xs, &amp_gamma)?;
    config.pool_state.price_state.xcp_profit_real =
        get_xcp(d, config.pool_state.price_state.price_scale)
            / (total_share - amount).to_decimal256(LP_TOKEN_PRECISION)?;

    let refund_assets = refund_assets
        .into_iter()
        .map(|asset| {
            let prec = precisions.get_precision(&asset.info).unwrap();

            Ok(Asset {
                info: asset.info,
                amount: asset.amount.to_uint(prec)?,
            })
        })
        .collect::<StdResult<Vec<_>>>()?;

    messages.extend(
        refund_assets
            .iter()
            .cloned()
            .map(|asset| asset.into_msg(&sender))
            .collect::<StdResult<Vec<_>>>()?,
    );
    messages.push(
        wasm_execute(
            &config.pair_info.liquidity_token,
            &Cw20ExecuteMsg::Burn { amount },
            vec![],
        )?
        .into(),
    );

    if config.track_asset_balances {
        for (i, pool) in pools.iter().enumerate() {
            BALANCES.save(
                deps.storage,
                &pool.info,
                &pool
                    .amount
                    .to_uint(precisions.get_precision(&pool.info)?)?
                    .checked_sub(refund_assets[i].amount)?,
                env.block.height,
            )?;
        }
    }

    //CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "withdraw_liquidity"),
        attr("sender", sender),
        attr("withdrawn_share", amount),
        attr("refund_assets", refund_assets.iter().join(", ")),
    ]))
}

#[allow(clippy::too_many_arguments)]
pub fn execute_create_pair(
    deps: &mut DepsMut,
    env: Env,
    _info: MessageInfo,
    init_params: Option<Binary>,
    asset_infos: Vec<AssetInfo>,
) -> Result<Response, ContractError> {
    if asset_infos.len() != 2 {
        return Err(StdError::generic_err("asset_infos must contain exactly two elements").into());
    }

    check_asset_infos(deps.api, &asset_infos)?;

    let params: ConcentratedPoolParams =
        from_binary(&init_params.ok_or(ContractError::InitParamsNotFound {})?)?;

    if params.price_scale.is_zero() {
        return Err(StdError::generic_err("Initial price scale can not be zero").into());
    }

    Precisions::store_precisions(deps.branch(), &asset_infos)?;

    let mut pool_params = PoolParams::default();
    pool_params.update_params(UpdatePoolParams {
        mid_fee: Some(params.mid_fee),
        out_fee: Some(params.out_fee),
        fee_gamma: Some(params.fee_gamma),
        repeg_profit_threshold: Some(params.repeg_profit_threshold),
        min_price_scale_delta: Some(params.min_price_scale_delta),
        ma_half_time: Some(params.ma_half_time),
    })?;

    let pool_state = PoolState {
        initial: AmpGamma::default(),
        future: AmpGamma::new(params.amp, params.gamma)?,
        future_time: env.block.time.seconds(),
        initial_time: 0,
        price_state: PriceState {
            oracle_price: params.price_scale.into(),
            last_price: params.price_scale.into(),
            price_scale: params.price_scale.into(),
            last_price_update: env.block.time.seconds(),
            xcp_profit: Decimal256::zero(),
            xcp_profit_real: Decimal256::zero(),
        },
    };

    let config = Config {
        pair_info: PairInfo {
            contract_addr: env.contract.address.clone(),
            liquidity_token: Addr::unchecked(""),
            asset_infos: asset_infos.clone(),
            pair_type: PairType::Custom("concentrated".to_string()),
        },
        factory_addr: Addr::unchecked(DUMMY_ADDRESS),
        pool_params,
        pool_state,
        owner: None,
        track_asset_balances: params.track_asset_balances.unwrap_or_default(),
        fee_share: None,
    };
    let mut balances = Vec::new();

    for info in &config.pair_info.asset_infos {
        balances.push(Asset {
            info: info.clone(),
            amount: Uint128::zero(),
        })
    }

    if config.track_asset_balances {
        for asset in &config.pair_info.asset_infos {
            BALANCES.save(deps.storage, asset, &Uint128::zero(), env.block.height)?;
        }
    }

    let key = generate_key_from_asset_info(&asset_infos);
    //println!("{:?}", key);
    POOLS.save(deps.storage, key.clone(), &config)?;
    PAIR_BALANCES.save(deps.storage, key.clone(), &balances)?;
    //BufferManager::init(deps.storage, OBSERVATIONS, OBSERVATIONS_SIZE)?;

    let token_name = format_lp_token_name(&asset_infos, &deps.querier)?;

    // Create LP token
    let sub_msg = SubMsg::reply_on_success(
        wasm_instantiate(
            2,
            &TokenInstantiateMsg {
                name: token_name,
                symbol: "pcLP".to_string(),
                decimals: LP_TOKEN_PRECISION,
                initial_balances: vec![],
                mint: Some(MinterResponse {
                    minter: env.contract.address.to_string(),
                    cap: None,
                }),
                marketing: None,
            },
            vec![],
            String::from("Pina Colada LP token"),
        )?,
        INSTANTIATE_TOKEN_REPLY_ID,
    );
    QUEUED_MINT.save(deps.storage, &key)?;
    Ok(Response::new().add_submessage(sub_msg).add_attribute(
        "asset_balances_tracking".to_owned(),
        if config.track_asset_balances {
            "enabled"
        } else {
            "disabled"
        }
        .to_owned(),
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn execute_swap_operations(
    deps: &mut DepsMut,
    env: Env,
    sender: Addr,
    operations: Vec<SwapOperation>,
    input_amount: Uint128,
    _minimum_receive: Option<Uint128>,
    to: Option<String>,
    max_spread: Option<Decimal>,
) -> Result<Response, ContractError> {
    assert_operations(deps.api, &operations)?;

    let recipient = addr_opt_validate(deps.api, &to)?.unwrap_or(sender);
    //let _target_asset_info = operations.last().unwrap().get_target_asset_info();
    let operations_len = operations.len();
    let mut messages = Vec::new();
    //initialize
    let mut return_amount = input_amount;

    for operation in operations.into_iter().enumerate() {
        let (offer_asset_info, ask_asset_info) =
            (operation.1.offer_asset_info, operation.1.ask_asset_info);
        if operation.0 == operations_len - 1 {
            let pool_key = generate_key_from_asset_info(
                &[offer_asset_info.clone(), ask_asset_info.clone()].to_vec(),
            );
            let offer_asset = Asset {
                info: offer_asset_info.clone(),
                amount: return_amount,
            };
            //println!("{} {}", "POOOOL", pool_key);
            let return_amount = swap_internal(
                deps,
                &env,
                pool_key,
                offer_asset,
                Some(Decimal::MAX),
                max_spread,
            )
            .unwrap();
            //println!("{} {}", "TRANSFERRING", return_amount);

            match ask_asset_info {
                AssetInfo::Token { contract_addr } => {
                    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: contract_addr.to_string(),
                        msg: to_binary(&Cw20ExecuteMsg::Transfer {
                            recipient: recipient.to_string(),
                            amount: return_amount,
                        })?,
                        funds: vec![],
                    }))
                }
                AssetInfo::NativeToken { .. } => {
                    messages.push(send_native(&recipient, return_amount).unwrap())
                }
            }
        } else {
            let pool_key = generate_key_from_asset_info(
                &[offer_asset_info.clone(), ask_asset_info.clone()].to_vec(),
            );
            let offer_asset = Asset {
                info: offer_asset_info.clone(),
                amount: return_amount,
            };
            //println!("{}", pool_key);
            let result = swap_internal(
                deps,
                &env,
                pool_key,
                offer_asset,
                Some(Decimal::MAX),
                max_spread,
            );

            return_amount = result.unwrap();
        }
    }

    Ok(Response::new().add_messages(messages))
}

/// Updates internal pools and calculated swap outputs The trader must approve the
/// pool contract to transfer offer assets from their wallet.
///
/// * **sender** is the sender of the swap operation.
///
/// /// * **pool_key** key of pool with offer and ask.
///
/// * **offer_asset** proposed asset for swapping.
///
/// * **belief_price** is used to calculate the maximum swap spread.
///
/// * **max_spread** sets the maximum spread of the swap operation.
///
/// * **to** sets the recipient of the swap operation.
fn swap_internal(
    deps: &mut DepsMut,
    env: &Env,
    pool_key: String,
    offer_asset: Asset,
    belief_price: Option<Decimal>,
    max_spread: Option<Decimal>,
) -> Result<Uint128, ContractError> {
    let precisions = Precisions::new(deps.storage)?;
    let offer_asset_prec = precisions.get_precision(&offer_asset.info)?;
    let offer_asset_dec = offer_asset.to_decimal_asset(offer_asset_prec)?;
    let offer_ind = find_asset_index(deps, pool_key.clone(), offer_asset.clone());
    let ask_ind = 1 ^ offer_ind;
    let mut config = POOLS.load(deps.storage, pool_key.clone())?;
    increment_asset_balance(deps, pool_key.clone(), offer_ind, offer_asset.amount);

    let mut pools = query_pools(&deps, &config, &precisions)?;

    let ask_asset_prec = precisions.get_precision(&pools[ask_ind].info)?;
    //println!("{},{}", pools[offer_ind].amount, "SUBTRACTION");
    pools[offer_ind].amount -= offer_asset_dec.amount;

    before_swap_check(&pools, offer_asset_dec.amount)?;

    let mut xs = pools.iter().map(|asset| asset.amount).collect_vec();
    //println!("{:?} {}", xs, "XS!!!!!!!!!!");

    let swap_result = compute_swap(
        &xs,
        offer_asset_dec.amount,
        ask_ind,
        &config,
        &env,
        Decimal256::zero(),
        Decimal256::zero(),
    )?;
    xs[offer_ind] += offer_asset_dec.amount;
    xs[ask_ind] -= swap_result.dy + swap_result.maker_fee + swap_result.share_fee;

    let return_amount = swap_result.dy.to_uint(ask_asset_prec)?;
    //println!("{:?} {}", return_amount, "RT AMT!!!!!!!!!!");
    let spread_amount = swap_result.spread_fee.to_uint(ask_asset_prec)?;
    assert_max_spread(
        belief_price,
        max_spread,
        offer_asset.amount,
        return_amount,
        spread_amount,
    )?;

    let total_share = query_supply(&deps.querier, &config.pair_info.liquidity_token)?
        .to_decimal256(LP_TOKEN_PRECISION)?;
    //println!("DECREASING");
    decrease_asset_balance(deps, pool_key.clone(), ask_ind, return_amount);
    // Skip very small trade sizes which could significantly mess up the price due to rounding errors,
    // especially if token precisions are 18.
    if (swap_result.dy + swap_result.maker_fee + swap_result.share_fee) >= MIN_TRADE_SIZE
        && offer_asset_dec.amount >= MIN_TRADE_SIZE
    {
        let last_price = swap_result.calc_last_price(offer_asset_dec.amount, offer_ind);

        // update_price() works only with internal representation
        xs[1] *= config.pool_state.price_state.price_scale;
        config
            .pool_state
            .update_price(&config.pool_params, &env, total_share, &xs, last_price)?;
    }

    /**let receiver = to.unwrap_or_else(|| sender.clone());

    let mut messages = vec![Asset {
        info: pools[ask_ind].info.clone(),
        amount: return_amount,
    }
    .into_msg(&receiver)?];

    // Send the shared fee
    let mut fee_share_amount = Uint128::zero();
    if let Some(fee_share) = config.fee_share.clone() {
        fee_share_amount = swap_result.share_fee.to_uint(ask_asset_prec)?;
        if !fee_share_amount.is_zero() {
            let fee = pools[ask_ind].info.with_balance(fee_share_amount);
            messages.push(fee.into_msg(fee_share.recipient)?);
        }
    }

    // Send the maker fee
    let mut maker_fee = Uint128::zero();
    if let Some(fee_address) = fee_info.fee_address {
        maker_fee = swap_result.maker_fee.to_uint(ask_asset_prec)?;
        if !maker_fee.is_zero() {
            let fee = pools[ask_ind].info.with_balance(maker_fee);
            messages.push(fee.into_msg(fee_address)?);
        }
    }
    **/
    // Store observation from precommit data
    //accumulate_swap_sizes(deps.storage, &env)?;

    // Store time series data in precommit observation.
    // Skipping small unsafe values which can seriously mess oracle price due to rounding errors.
    // This data will be reflected in observations in the next action.
    if offer_asset_dec.amount >= MIN_TRADE_SIZE && swap_result.dy >= MIN_TRADE_SIZE {
        let (base_amount, quote_amount) = if offer_ind == 0 {
            (offer_asset.amount, return_amount)
        } else {
            (return_amount, offer_asset.amount)
        };
        PrecommitObservation::save(deps.storage, &env, base_amount, quote_amount)?;
    }

    POOLS.save(deps.storage, pool_key, &config)?;

    Ok(return_amount)
}
