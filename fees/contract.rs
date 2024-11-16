use cosmwasm_std::{
    entry_point, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
    Uint128, CosmosMsg, BankMsg,
};
use cw2::set_contract_version;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const CONTRACT_NAME: &str = "crates.io:fee-collector";
const CONTRACT_VERSION: &str = "1.0.0";

// Initial cost in tokens to collect fees
const INITIAL_COLLECTION_COST: u128 = 20_000;
// Base cost that we'll never go below (e.g., 100 tokens)
const MIN_COLLECTION_COST: u128 = 100;
// Decay factor for cost reduction (smaller = slower decay)
const DECAY_FACTOR: f64 = 0.1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub admin: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub admin: String,
    pub total_collections: u64,
    pub fees_collected: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    CollectFees {},
    AddFees {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    GetState {},
    GetCollectionCost {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct StateResponse {
    pub admin: String,
    pub current_collection_cost: Uint128,
    pub total_collections: u64,
    pub fees_collected: Uint128,
}

// Calculate collection cost based on number of collections
// Uses an exponential decay function that approaches MIN_COLLECTION_COST asymptotically
fn calculate_collection_cost(collections: u64) -> Uint128 {
    let collections_f64 = collections as f64;
    let decay = (-DECAY_FACTOR * collections_f64).exp();
    let variable_portion = (INITIAL_COLLECTION_COST - MIN_COLLECTION_COST) as f64;
    
    let cost = (variable_portion * decay) as u128 + MIN_COLLECTION_COST;
    Uint128::new(cost)
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    deps.api.addr_validate(&msg.admin)?;

    let state = State {
        admin: msg.admin,
        total_collections: 0,
        fees_collected: Uint128::zero(),
    };
    
    deps.storage.set(b"state", &serde_json::to_vec(&state)?);
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("initial_collection_cost", INITIAL_COLLECTION_COST.to_string()))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> StdResult<Response> {
    match msg {
        ExecuteMsg::CollectFees {} => execute_collect_fees(deps, env, info),
        ExecuteMsg::AddFees {} => execute_add_fees(deps, info),
    }
}

pub fn execute_collect_fees(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> StdResult<Response> {
    let mut state: State = serde_json::from_slice(&deps.storage.get(b"state").unwrap())?;
    
    // Calculate current collection cost based on total collections
    let required_tokens = calculate_collection_cost(state.total_collections);
    
    let sent_tokens = info.funds.iter()
        .find(|coin| coin.denom == "token")
        .map(|coin| coin.amount)
        .unwrap_or_else(Uint128::zero);

    if sent_tokens < required_tokens {
        return Err(cosmwasm_std::StdError::generic_err(format!(
            "Insufficient tokens sent. Required: {}, Sent: {}",
            required_tokens, sent_tokens
        )));
    }

    // Create bank send message to send all collected fees to collector
    let send_fees_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![cosmwasm_std::Coin {
            denom: "token".to_string(),
            amount: state.fees_collected,
        }],
    });

    // Update state
    state.total_collections += 1;
    state.fees_collected = Uint128::zero();
    
    // Save updated state
    deps.storage.set(b"state", &serde_json::to_vec(&state)?);

    // Calculate next collection cost for the attribute
    let next_cost = calculate_collection_cost(state.total_collections);

    Ok(Response::new()
        .add_message(send_fees_msg)
        .add_attribute("action", "collect_fees")
        .add_attribute("collector", info.sender)
        .add_attribute("amount_collected", state.fees_collected)
        .add_attribute("tokens_burned", required_tokens)
        .add_attribute("next_collection_cost", next_cost))
}

pub fn execute_add_fees(
    deps: DepsMut,
    info: MessageInfo,
) -> StdResult<Response> {
    let mut state: State = serde_json::from_slice(&deps.storage.get(b"state").unwrap())?;
    
    let sent_amount = info.funds.iter()
        .find(|coin| coin.denom == "token")
        .map(|coin| coin.amount)
        .unwrap_or_else(Uint128::zero);

    state.fees_collected += sent_amount;
    
    deps.storage.set(b"state", &serde_json::to_vec(&state)?);

    Ok(Response::new()
        .add_attribute("action", "add_fees")
        .add_attribute("amount_added", sent_amount))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetState {} => query_state(deps),
        QueryMsg::GetCollectionCost {} => query_collection_cost(deps),
    }
}

fn query_state(deps: Deps) -> StdResult<Binary> {
    let state: State = serde_json::from_slice(&deps.storage.get(b"state").unwrap())?;
    let current_cost = calculate_collection_cost(state.total_collections);
    
    to_binary(&StateResponse {
        admin: state.admin.clone(),
        current_collection_cost: current_cost,
        total_collections: state.total_collections,
        fees_collected: state.fees_collected,
    })
}

fn query_collection_cost(deps: Deps) -> StdResult<Binary> {
    let state: State = serde_json::from_slice(&deps.storage.get(b"state").unwrap())?;
    let cost = calculate_collection_cost(state.total_collections);
    to_binary(&cost)
}