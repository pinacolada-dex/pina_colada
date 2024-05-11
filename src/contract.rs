use astroport::asset::{addr_opt_validate, Asset, AssetInfo};
use astroport::pair::PoolResponse;
use astroport::querier::query_supply;


use astroport_pcl_common::utils::check_cw20_in_pool;
use cosmwasm_std::{
    entry_point, from_binary, to_binary, Addr, Api, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult, SubMsgResponse, SubMsgResult, Uint128
};
use cw2::{get_contract_version, set_contract_version};
use cw20::Cw20ReceiveMsg;
use cw_utils::{must_pay, parse_instantiate_response_data};

use crate::msg::SwapOperation;

use astroport::router::{
    InstantiateMsg, MigrateMsg,
};

use crate::msg::{ExecuteMsg,QueryMsg,Cw20HookMsg};
use crate::error::ContractError;
use crate::handlers::{execute_create_pair, execute_provide_liquidity, execute_swap_operations, execute_withdraw_liquidity, generate_key_from_asset_info, generate_key_from_assets, DENOM};

use crate::query::{query_compute_d, query_lp_price, simulate_swap_operations,query_config};
use crate::state::{ PAIR_BALANCES, POOLS, QUEUED_MINT};

/// Contract name that is used for migration.
const CONTRACT_NAME: &str = "pina-colada";
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const AFTER_SWAP_REPLY_ID: u64 = 1;

/// Creates a new contract with the specified parameters in the [`InstantiateMsg`].
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;



    Ok(Response::default())
}

/// Exposes all the execute functions available in the contract.
///
/// ## Variants
/// * **ExecuteMsg::Receive(msg)** Receives a message of type [`Cw20ReceiveMsg`] and processes
/// it depending on the received template.
///
/// * **ExecuteMsg::ExecuteSwapOperations {
///             operations,
///             minimum_receive,
///             to
///         }** Performs swap operations with the specified parameters.
///
/// * **ExecuteMsg::ExecuteSwapOperation { operation, to }** Execute a single swap operation.
///
/// * **ExecuteMsg::AssertMinimumReceive {
///             asset_info,
///             prev_balance,
///             minimum_receive,
///             receiver
///         }** Checks if an ask amount is higher than or equal to the minimum amount to receive.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    mut deps:DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Receive(msg) => receive_cw20(&mut deps, env,info, msg),
        ExecuteMsg::ExecuteSwapOperations {
            operations,
            minimum_receive,
            to,
            max_spread,
        } => {
            assert_eq!(operations[0].clone().offer_asset_info,AssetInfo::NativeToken { denom: String::from(DENOM) });
            let amount=must_pay(&info,DENOM).unwrap();
            assert!(!amount.is_zero(),"Cannot Swap with Zero Input");
            //println!("{} {}",amount,"VALUE to be Swapped");
            execute_swap_operations(
            &mut deps,
            env,
            info.sender.clone(),
            operations,
            amount,
            minimum_receive,
            to,
            max_spread,
        )
        },         
         
        ExecuteMsg::CreatePair{asset_infos,token_code_id: _,init_params}=>execute_create_pair(&mut deps, env, info,init_params,asset_infos),
        
        ExecuteMsg::ProvideLiquidity{assets,slippage_tolerance,auto_stake,receiver}=>execute_provide_liquidity(&mut deps, env, info,assets,slippage_tolerance,auto_stake,receiver),
       // ExecuteMsg::WithdrawLiquidity{assets,amount}=>execute_withdraw_liquidity(&mut deps,env,info.clone(),info.sender.clone(),amount,assets),
    }  
}

pub fn receive_cw20(
    deps: &mut DepsMut,
    env: Env,
    info:MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    match from_binary(&cw20_msg.msg)? {
        Cw20HookMsg::ExecuteSwapOperations {
            operations,
            minimum_receive,
            to,
            max_spread,
        } => {
            //println!("{} is {}",info.sender.clone(),String::from("Test"));
            
            let pool_key=generate_key_from_asset_info(&[operations[0].clone().offer_asset_info,operations[0].clone().ask_asset_info].to_vec());
            let config=POOLS.may_load(deps.storage, pool_key.clone()).unwrap();

            // Only asset contract can execute this message
            check_cw20_in_pool(&config.unwrap(), &info.sender)?;

            let to_addr = addr_opt_validate(deps.api, &to)?;
            execute_swap_operations(
            deps,
            env,
            Addr::unchecked(cw20_msg.sender),
            operations,
            cw20_msg.amount,
            minimum_receive,
            to,
            max_spread,
            )
        },
        
        Cw20HookMsg::WithdrawLiquidity { assets } => execute_withdraw_liquidity(deps,env,info.clone(),info.sender.clone(),cw20_msg.amount,assets)
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg {
        Reply {
            id: _INSTANTIATE_TOKEN_REPLY_ID,
            result:
                SubMsgResult::Ok(SubMsgResponse {
                    data: Some(data), ..
                }),
        } => {
            let pool_key= QUEUED_MINT.load(deps.storage).unwrap();
            let config=POOLS.may_load(deps.storage, pool_key.clone()).unwrap();
            let init_response = parse_instantiate_response_data(data.as_slice())
            .map_err(|e| StdError::generic_err(format!("{e}")))?;
            if let Some(mut config)=config{
                config.pair_info.liquidity_token =
                deps.api.addr_validate(&init_response.contract_address)?;
                POOLS.save(deps.storage,pool_key ,&config)?;
                QUEUED_MINT.remove(deps.storage);
                Ok(Response::new()
                .add_attribute("liquidity_token_addr", config.pair_info.liquidity_token))
               //return  Err(ContractError::FailedToParseReply {})
            }else{
                return  Err(ContractError::FailedToParseReply {})
            }
                  
        }
        _ => Err(ContractError::FailedToParseReply {}),
    }
}


/// Exposes all the queries available in the contract.
/// ## Queries
/// * **QueryMsg::Config {}** Returns general router parameters using a [`ConfigResponse`] object.
/// * **QueryMsg::SimulateSwapOperations {
///             offer_amount,
///             operations,
///         }** Simulates one or multiple swap operations and returns the end result in a [`SimulateSwapOperationsResponse`] object.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
       
        QueryMsg::SimulateSwapOperations {
            offer_amount,
            operations,
        } => Ok(to_binary(&simulate_swap_operations(
            deps,
            env,
            offer_amount,
            operations,
        )?)?),
        QueryMsg::Pool {pool_key} => Ok(to_binary(&query_pool(deps,pool_key)?)?),
        QueryMsg::Pair {pool_key} => Ok(to_binary(&POOLS.load(deps.storage,pool_key)?.pair_info)?),
        QueryMsg::ComputeD { pool_key }=>Ok(to_binary(&query_compute_d(deps,env,pool_key)?)?),
        QueryMsg::Config {pool_key  }=> Ok(to_binary(&query_config(deps,env,pool_key)?)?),
        QueryMsg::LpPrice {pool_key  }=>Ok(to_binary(&query_lp_price(deps,env,pool_key)?)?),
}
}
fn query_pool(deps: Deps,pool_key:String)->StdResult<PoolResponse>{
    let config= POOLS.load(deps.storage,pool_key.clone())?;
    let assets= PAIR_BALANCES.load(deps.storage,pool_key.clone())?;
    let total_share = query_supply(&deps.querier, &config.pair_info.liquidity_token)?;
    let resp = PoolResponse {
        assets,
        total_share,
    };

    Ok(resp)
}



/// Manages contract migration.
#[cfg(not(tarpaulin_include))]
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let contract_version = get_contract_version(deps.storage)?;

    match contract_version.contract.as_ref() {
        "router" => match contract_version.version.as_ref() {
            "1.1.1" => {}
            _ => return Err(ContractError::MigrationError {}),
        },
        _ => return Err(ContractError::MigrationError {}),
    };

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new()
        .add_attribute("previous_contract_name", &contract_version.contract)
        .add_attribute("previous_contract_version", &contract_version.version)
        .add_attribute("new_contract_name", CONTRACT_NAME)
        .add_attribute("new_contract_version", CONTRACT_VERSION))
}

