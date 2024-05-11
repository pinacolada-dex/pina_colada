#![cfg(not(tarpaulin_include))]

use std::error::Error;
use std::fmt::Display;
use std::ops::Add;
use std::str::FromStr;

use crate::error::ContractError;
use crate::factory_helper::{instantiate_token, mint, mint_native, FactoryHelper};
use crate::msg::Cw20HookMsg;
use crate::msg::ExecuteMsg;
use crate::msg::SwapOperation;
use astroport::asset::{
    native_asset, native_asset_info, token_asset, token_asset_info, Asset, AssetInfo, PairInfo,
};
use astroport::factory::PairType;
use astroport::pair::PoolResponse;
use astroport::pair_concentrated::{
    ConcentratedPoolConfig, ConcentratedPoolParams, ConcentratedPoolUpdateParams,
};
use crate::msg::QueryMsg;
use astroport::router::{InstantiateMsg, MigrateMsg};
use astroport::token;
use cosmwasm_std::{coins, from_binary, to_binary, Addr, Coin, Decimal, Empty, StdError, Uint128};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};
pub static DENOM: &str = "aarch";
pub fn common_pcl_params() -> ConcentratedPoolParams {
    ConcentratedPoolParams {
        amp: f64_to_dec(40f64),
        gamma: f64_to_dec(0.000145),
        mid_fee: f64_to_dec(0.0026),
        out_fee: f64_to_dec(0.0045),
        fee_gamma: f64_to_dec(0.00023),
        repeg_profit_threshold: f64_to_dec(0.000002),
        min_price_scale_delta: f64_to_dec(0.000146),
        price_scale: Decimal::one(),
        ma_half_time: 600,
        track_asset_balances: None,
        fee_share: None,
    }
}
pub fn f64_to_dec<T>(val: f64) -> T
where
    T: FromStr,
    T::Err: Error,
{
    T::from_str(&val.to_string()).unwrap()
}

pub fn dec_to_f64(val: impl Display) -> f64 {
    f64::from_str(&val.to_string()).unwrap()
}

fn router_contract() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        )
        .with_reply_empty(crate::contract::reply),
    )
}

#[test]
fn pool_manager_works() {
    let mut app = App::default();

    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");

    let router_code = app.store_code(router_contract());
    let pool_manager = app
        .instantiate_contract(
            router_code,
            owner.clone(),
            &InstantiateMsg {
                astroport_factory: String::from("Pina_Colada"),
            },
            &[],
            "router",
            None,
        )
        .unwrap();

    let mut helper = FactoryHelper::init(&mut app, &owner, &pool_manager);
    let token_x = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOX", None);
    let token_y = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOY", None);
    let token_z = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOZ", None);
    println!("{}", pool_manager);
    println!("{}", token_x);
    println!("{}", token_y);
    println!("{}", token_z);
    let params = ConcentratedPoolParams {
        price_scale: Decimal::from_ratio(1u8, 2u8),
        ..common_pcl_params()
    };
    for (a, b, typ, liq) in [
        (&token_x, &token_y, PairType::Xyk {}, 800_000_000000),
        (&token_y, &token_z, PairType::Stable {}, 900_000_000000),
    ] {
        let params = Some(to_binary(&params).unwrap());
        let pair = helper
            .create_pair(
                &mut app,
                &owner,
                [token_asset_info(a.clone()), token_asset_info(b.clone())],
                params,
            )
            .unwrap();
        mint(&mut app, &owner, a, liq, &owner).unwrap();
        mint(&mut app, &owner, b, liq, &owner).unwrap();
        mint(&mut app, &owner, a, liq, &user).unwrap();
        mint(&mut app, &owner, b, liq, &user).unwrap();
    }
    let n = 10_000_00000u128;
    let assets1 = [
        token_asset(token_x.clone(), n.into()),
        token_asset(token_y.clone(), n.into()),
    ]
    .to_vec();
    let assets2 = [
        token_asset(token_y.clone(), n.into()),
        token_asset(token_z.clone(), n.into()),
    ]
    .to_vec();
    let provide_msg = ExecuteMsg::ProvideLiquidity {
        assets: assets1,
        slippage_tolerance: Some(f64_to_dec(0.5)),
        auto_stake: None,
        receiver: None,
    };
    let provide_msg2 = ExecuteMsg::ProvideLiquidity {
        assets: assets2,
        slippage_tolerance: Some(f64_to_dec(0.5)),
        auto_stake: None,
        receiver: None,
    };
    let msg = Cw20ExecuteMsg::IncreaseAllowance {
        spender: pool_manager.to_string(),
        expires: None,
        amount: (100 * n).into(),
    };

    app.execute_contract(owner.clone(), token_x.clone(), &msg, &[])
        .unwrap();

    app.execute_contract(owner.clone(), token_y.clone(), &msg, &[])
        .unwrap();
    app.execute_contract(owner.clone(), token_z.clone(), &msg, &[])
        .unwrap();

    app.execute_contract(owner.clone(), pool_manager.clone(), &provide_msg, &[])
        .unwrap();
    //app.execute_contract(owner.clone(), pool_manager.clone(), &provide_msg, &[]).unwrap();
    app.execute_contract(owner.clone(), pool_manager.clone(), &provide_msg2, &[])
        .unwrap();

    let swap_msg = Cw20ExecuteMsg::Send {
        contract: pool_manager.clone().to_string(),
        amount: Uint128::from(10000u128),
        msg: to_binary(&Cw20HookMsg::ExecuteSwapOperations {
            operations: vec![
                SwapOperation {
                    offer_asset_info: AssetInfo::Token {
                        contract_addr: token_x.clone(),
                    },
                    ask_asset_info: AssetInfo::Token {
                        contract_addr: token_y.clone(),
                    },
                },
                SwapOperation {
                    offer_asset_info: AssetInfo::Token {
                        contract_addr: token_y.clone(),
                    },
                    ask_asset_info: AssetInfo::Token {
                        contract_addr: token_z,
                    },
                },
            ],
            minimum_receive: None,
            to: None,
            max_spread: None,
        })
        .unwrap(),
    };
    let pool_key=format!("{}{}",token_x,token_y);
    println!("{} {}","querying pool at ",pool_key);
    let pool_data:PoolResponse=app.wrap().query_wasm_smart(pool_manager.clone(), &QueryMsg::Pool {pool_key:pool_key.clone()}).unwrap();
    println!("{:?}",pool_data);
    let pair_data:PairInfo=app.wrap().query_wasm_smart(pool_manager.clone(), &QueryMsg::Pair {pool_key}).unwrap();
    println!("{:?}",pair_data);
    let withdraw_liq_msg = Cw20HookMsg::WithdrawLiquidity {
        assets: [
            Asset {
                info: AssetInfo::Token {
                    contract_addr: token_x.clone(),
                },
                /// A token amount
                amount: Uint128::from(1000000_u128),
            },
            Asset {
                info: AssetInfo::Token {
                    contract_addr: token_y.clone(),
                },
                /// A token amount
                amount: Uint128::from(1000000_u128),
            },
        ]
        .to_vec(),
    };
    let withdraw_msg = Cw20ExecuteMsg::Send {
        contract: pool_manager.clone().to_string(),
        amount: Uint128::from(1000000_u128),
        msg: to_binary(&withdraw_liq_msg).unwrap(),
    };
    app.execute_contract(
        owner.clone(),
        Addr::unchecked("contract4"),
        &withdraw_msg,
        &[],
    )
    .unwrap();
}

#[test]
fn test_native_to_token_swap() {
    let mut app = App::default();

    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");

    let router_code = app.store_code(router_contract());
    let pool_manager = app
        .instantiate_contract(
            router_code,
            owner.clone(),
            &InstantiateMsg {
                astroport_factory: String::from("Pina_Colada"),
            },
            &[],
            "router",
            None,
        )
        .unwrap();

    let mut helper = FactoryHelper::init(&mut app, &owner, &pool_manager);
    let token_x = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOX", None);
    let token_y = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOY", None);
    let token_z = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOZ", None);
    println!("{}", pool_manager);
    println!("{}", token_x);
    println!("{}", token_y);
    println!("{}", token_z);
    let params = ConcentratedPoolParams {
        price_scale: Decimal::from_ratio(1u8, 2u8),
        ..common_pcl_params()
    };
    for (a, b, typ, liq) in [
        (&token_x, &token_y, PairType::Xyk {}, 800_000_00000000),
        (&token_y, &token_z, PairType::Stable {}, 900_000_00000000),
    ] {
        let params = Some(to_binary(&params).unwrap());
        let pair = helper
            .create_pair(
                &mut app,
                &owner,
                [token_asset_info(a.clone()), token_asset_info(b.clone())],
                params,
            )
            .unwrap();
        mint(&mut app, &owner, a, liq, &owner).unwrap();
        mint(&mut app, &owner, b, liq, &owner).unwrap();
        mint(&mut app, &owner, a, liq, &user).unwrap();
        mint(&mut app, &owner, b, liq, &user).unwrap();
    }
    let n = 10_000_00000u128;
    let msg = Cw20ExecuteMsg::IncreaseAllowance {
        spender: pool_manager.to_string(),
        expires: None,
        amount: (100 * n).into(),
    };

    app.execute_contract(owner.clone(), token_x.clone(), &msg, &[])
        .unwrap();
    let arch = n * 1000000000000000;
    let r = mint_native(&mut app, DENOM, 10 * arch, &owner);
    let pair = helper
        .create_pair(
            &mut app,
            &owner,
            [
                native_asset_info(String::from(DENOM)),
                token_asset_info(token_x.clone()),
            ],
            Some(to_binary(&params).unwrap()),
        )
        .unwrap();
    let n = 10_000_00000u128;

    let assets1 = [
        token_asset(token_x.clone(), n.into()),
        native_asset(String::from(DENOM), arch.into()),
    ]
    .to_vec();

    let provide_msg = ExecuteMsg::ProvideLiquidity {
        assets: assets1,
        slippage_tolerance: Some(f64_to_dec(0.5)),
        auto_stake: None,
        receiver: None,
    };
    app.execute_contract(
        owner.clone(),
        pool_manager.clone(),
        &provide_msg,
        &[Coin {
            denom: String::from(DENOM),
            amount: arch.into(),
        }],
    )
    .unwrap();
    let swap_msg = ExecuteMsg::ExecuteSwapOperations {
        operations: vec![SwapOperation {
            offer_asset_info: AssetInfo::NativeToken {
                denom: String::from(DENOM),
            },
            ask_asset_info: AssetInfo::Token {
                contract_addr: token_x.clone(),
            },
        }],
        minimum_receive: None,
        to: None,
        max_spread: None,
    };
    app.execute_contract(
        owner.clone(),
        pool_manager.clone(),
        &swap_msg,
        &[Coin {
            denom: String::from(DENOM),
            amount: (1000000000 * n).into(),
        }],
    )
    .unwrap();
}
#[test]
fn test_token_to_native_swap() {
    let mut app = App::default();

    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");

    let router_code = app.store_code(router_contract());
    let pool_manager = app
        .instantiate_contract(
            router_code,
            owner.clone(),
            &InstantiateMsg {
                astroport_factory: String::from("Pina_Colada"),
            },
            &[],
            "router",
            None,
        )
        .unwrap();

    let mut helper = FactoryHelper::init(&mut app, &owner, &pool_manager);
    let token_x = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOX", None);
    let token_y = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOY", None);
    let token_z = instantiate_token(&mut app, helper.cw20_token_code_id, &owner, "TOZ", None);
    println!("{}", pool_manager);
    println!("{}", token_x);
    println!("{}", token_y);
    println!("{}", token_z);
    let params = ConcentratedPoolParams {
        price_scale: Decimal::from_ratio(1u8, 2u8),
        ..common_pcl_params()
    };
    for (a, b, typ, liq) in [
        (&token_x, &token_y, PairType::Xyk {}, 800_000_000_000_000),
        (&token_y, &token_z, PairType::Stable {}, 900_000_000000_000),
    ] {
        let params = Some(to_binary(&params).unwrap());
        let pair = helper
            .create_pair(
                &mut app,
                &owner,
                [token_asset_info(a.clone()), token_asset_info(b.clone())],
                params,
            )
            .unwrap();
        mint(&mut app, &owner, a, liq, &owner).unwrap();
        mint(&mut app, &owner, b, liq, &owner).unwrap();
        mint(&mut app, &owner, a, liq, &user).unwrap();
        mint(&mut app, &owner, b, liq, &user).unwrap();
    }
    let n = 10_000_00000u128;
    let msg = Cw20ExecuteMsg::IncreaseAllowance {
        spender: pool_manager.to_string(),
        expires: None,
        amount: (10000000000000 * n).into(),
    };

    app.execute_contract(owner.clone(), token_x.clone(), &msg, &[])
        .unwrap();
    let arch = n * 1000000000000000;
    let r = mint_native(&mut app, DENOM, 10 * arch, &owner);
    let pair = helper
        .create_pair(
            &mut app,
            &owner,
            [
                native_asset_info(String::from(DENOM)),
                token_asset_info(token_x.clone()),
            ],
            Some(to_binary(&params).unwrap()),
        )
        .unwrap();
    let m = 100_000_000_000_000u128;

    let assets1 = [
        native_asset(String::from(DENOM), arch.into()),
        token_asset(token_x.clone(), m.into()),
    ]
    .to_vec();

    let provide_msg = ExecuteMsg::ProvideLiquidity {
        assets: assets1,
        slippage_tolerance: Some(f64_to_dec(0.5)),
        auto_stake: None,
        receiver: None,
    };
    app.execute_contract(
        owner.clone(),
        pool_manager.clone(),
        &provide_msg,
        &[Coin {
            denom: String::from(DENOM),
            amount: arch.into(),
        }],
    )
    .unwrap();
    let swap_msg = Cw20ExecuteMsg::Send {
        contract: pool_manager.clone().to_string(),
        amount: Uint128::from(1000000000u128),
        msg: to_binary(&Cw20HookMsg::ExecuteSwapOperations {
            operations: vec![SwapOperation {
                offer_asset_info: AssetInfo::Token {
                    contract_addr: token_x.clone(),
                },
                ask_asset_info: AssetInfo::NativeToken {
                    denom: String::from(DENOM),
                },
            }],
            minimum_receive: None,
            to: None,
            max_spread: None,
        })
        .unwrap(),
    };
    app.execute_contract(owner.clone(), token_x.clone(), &swap_msg, &[])
        .unwrap();
}
