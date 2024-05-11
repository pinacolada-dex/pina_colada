#![cfg(not(tarpaulin_include))]


use anyhow::Result as AnyResult;
use cosmwasm_std::{coins, Addr, Binary, Decimal};
use cw20::MinterResponse;
use cw_multi_test::{App, AppResponse, ContractWrapper, Executor};

use astroport::asset::{Asset, AssetInfo, PairInfo};
use astroport::factory::{PairConfig, PairType, QueryMsg};
use crate::msg::ExecuteMsg::{self, CreatePair};
pub struct FactoryHelper {
    pub owner: Addr,   
    pub pool_manager:Addr,
    pub cw20_token_code_id: u64,
}

impl FactoryHelper {
    pub fn init(router: &mut App, owner: &Addr,pool_manager:&Addr) -> Self {
        
        let astro_token_contract = Box::new(ContractWrapper::new_with_empty(
            astroport_token::contract::execute,
            astroport_token::contract::instantiate,
            astroport_token::contract::query,
        ));

        let cw20_token_code_id = router.store_code(astro_token_contract);
        Self{
            pool_manager:pool_manager.clone(),
            owner:owner.clone(),
            cw20_token_code_id:cw20_token_code_id
        }
    }
   
    pub fn create_pair(
        &mut self,
        router: &mut App,
        sender: &Addr,
       
        asset_infos: [AssetInfo; 2],
        init_params: Option<Binary>,
    ) -> AnyResult<Addr> {
        let msg = CreatePair {
            
            asset_infos: asset_infos.to_vec(),
            token_code_id:self.cw20_token_code_id,
            init_params,
        };

        router.execute_contract(sender.clone(), self.pool_manager.clone(), &msg, &[])?;

        /**let res: PairInfo = router.wrap().query_wasm_smart(
            self.pool_manager.clone(),
            &QueryMsg::Pair {
                asset_infos: asset_infos.to_vec(),
            },
        )?;
        **/
        Ok(self.pool_manager.clone())
    }
    pub fn provide_liquidity_with_slip_tolerance(
        &mut self,
        router: &mut App,
        sender: &Addr,
        assets: &[Asset],
        slippage_tolerance: Option<Decimal>,
    ) -> AnyResult<AppResponse> {
       

        let msg = ExecuteMsg::ProvideLiquidity {
            assets: assets.clone().to_vec(),
            slippage_tolerance,
            auto_stake: None,
            receiver: None,
        };

        
        router.execute_contract(sender.clone(), self.pool_manager.clone(), &msg, &[])
    }
}
  
pub fn instantiate_token(
    app: &mut App,
    token_code_id: u64,
    owner: &Addr,
    token_name: &str,
    decimals: Option<u8>,
) -> Addr {
    let init_msg = astroport::token::InstantiateMsg {
        name: token_name.to_string(),
        symbol: token_name.to_string(),
        decimals: decimals.unwrap_or(6),
        initial_balances: vec![],
        mint: Some(MinterResponse {
            minter: owner.to_string(),
            cap: None,
        }),
        marketing: None,
    };

    app.instantiate_contract(
        token_code_id,
        owner.clone(),
        &init_msg,
        &[],
        token_name,
        None,
    )
    .unwrap()
}

pub fn mint(
    app: &mut App,
    owner: &Addr,
    token: &Addr,
    amount: u128,
    receiver: &Addr,
) -> AnyResult<AppResponse> {
    app.execute_contract(
        owner.clone(),
        token.clone(),
        &cw20::Cw20ExecuteMsg::Mint {
            recipient: receiver.to_string(),
            amount: amount.into(),
        },
        &[],
    )
}

pub fn mint_native(
    app: &mut App,
    denom: &str,
    amount: u128,
    receiver: &Addr,
) -> AnyResult<AppResponse> {
    // .init_balance() erases previous balance thus we use such hack and create intermediate "denom admin"
    let denom_admin = Addr::unchecked(format!("{denom}_admin"));
    let coins_vec = coins(amount, denom);
    app.init_modules(|router, _, storage| {
        router
            .bank
            .init_balance(storage, &denom_admin, coins_vec.clone())
    })
    .unwrap();

    app.send_tokens(denom_admin, receiver.clone(), &coins_vec)
}

