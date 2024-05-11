use astroport_pcl_common::consts::MIN_AMP_CHANGING_TIME;
use cosmwasm_std::{Decimal,ConversionOverflowError, OverflowError, StdError, Uint128};
use astroport::{asset::MINIMUM_LIQUIDITY_AMOUNT, pair::MAX_FEE_SHARE_BPS};

use astroport_pcl_common::error::PclError;
use thiserror::Error;
#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    PclError(#[from] PclError),

    #[error("{0} parameter must be greater than {1} and less than or equal to {2}")]
    IncorrectPoolParam(String, String, String),
    
    #[error("{0}")]
    OverflowError(#[from] OverflowError),
    #[error("{0}")]
    ConversionOverflowError(#[from] ConversionOverflowError),

    #[error("Unauthorized")]
    Unauthorized{},
    #[error("Contract can't be migrated!")]
    MigrationError {},
    #[error("You need to provide init params")]
    InitParamsNotFound {},
    #[error("Initial liquidity must be more than {}", MINIMUM_LIQUIDITY_AMOUNT)]
    MinimumLiquidityAmountError {},

    #[error(
    "{0} error: The difference between the old and new amp or gamma values must not exceed {1} percent",
    )]
    MaxChangeAssertion(String, Decimal),
    #[error(
        "Fee share is 0 or exceeds maximum allowed value of {} bps",
        MAX_FEE_SHARE_BPS
    )]
    FeeShareOutOfBounds {},
    #[error(
        "Amp and gamma coefficients cannot be changed more often than once per {} seconds",
        MIN_AMP_CHANGING_TIME
    )]
    MinChangingTimeAssertion {},

    #[error("Doubling assets in asset infos")]
    DoublingAssets {},

   

    #[error("Generator address is not set in factory. Cannot auto-stake")]
    AutoStakeError {},

    #[error("Operation exceeds max spread limit")]
    MaxSpreadAssertion {},

    #[error("Provided spread amount exceeds allowed limit")]
    AllowedSpreadAssertion {},

    #[error("The asset {0} does not belong to the pair")]
    InvalidAsset(String),
    #[error(
        "The next offer asset must be the same as the previous ask asset; \
    {prev_ask_asset} --> {next_offer_asset} --> {next_ask_asset}"
    )]
    InvalidPathOperations {
        prev_ask_asset: String,
        next_offer_asset: String,
        next_ask_asset: String,
    },

    #[error("Doubling assets in one batch of path; {offer_asset} --> {ask_asset}")]
    DoublingAssetsPath {
        offer_asset: String,
        ask_asset: String,
    },

    #[error("Must specify swap operations!")]
    MustProvideOperations {},

    #[error("Assertion failed; minimum receive amount: {receive}, swap amount: {amount}")]
    AssertionMinimumReceive { receive: Uint128, amount: Uint128 },

    #[error("The swap operation limit was exceeded!")]
    SwapLimitExceeded {},

    #[error("Native swap operations are not supported!")]
    NativeSwapNotSupported {},
    #[error("")]
    InvalidZeroAmount{},
    #[error("Invalid number of assets. This pair supports only {0} assets")]
    InvalidNumberOfAssets(usize),
    #[error("Failed to Parse Reply")]
    FailedToParseReply{},
}