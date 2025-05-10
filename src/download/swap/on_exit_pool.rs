use crate::download::swap::{EURE_ARRAY_INDEX, SDAI_ARRAY_INDEX, Swap, compute_sdai_eure_from_bpt};
use crate::helper::{Position, StateBySubPath};
use alloy::primitives::{B256, U256, keccak256};
use alloy::rpc::types::trace::parity::{CallAction, TraceOutput};
use alloy::sol;
use alloy::sol_types::SolCall;
use eyre::{OptionExt, Result, WrapErr, eyre};
use log::debug;

sol!(
    #[derive(Debug, PartialEq, Eq)]
    function onExitPool(bytes32 poolId, address sender, address recipient, uint256[] memory balances, uint256 lastChangeBlock, uint256 protocolSwapFeePercentage, bytes memory userData) external override returns (uint256[] memory, uint256[] memory);
);

#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
enum ExitKind {
    ExactBptInForOneTokenOut,
    BptInForExactTokensOut,
    ExactBptInForAllTokensOut,
}
impl TryFrom<&[u8]> for ExitKind {
    type Error = eyre::Error;
    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        match value.last() {
            Some(0) => Ok(ExitKind::ExactBptInForOneTokenOut),
            Some(1) => Ok(ExitKind::BptInForExactTokensOut),
            Some(2) => Ok(ExitKind::ExactBptInForAllTokensOut),
            _ => Err(eyre::eyre!("Unknown exit kind: {:?}", value)),
        }
    }
}

pub fn decode_in_out_on_exit_pool(
    call_action: &CallAction,
    trace_output: &TraceOutput,
) -> Result<Option<(onExitPoolCall, onExitPoolReturn)>> {
    let Ok(exit_pool_in) = onExitPoolCall::abi_decode(&call_action.input) else {
        return Ok(None);
    };
    let Ok(exit_pool_out) = onExitPoolCall::abi_decode_returns(trace_output.output()) else {
        return Ok(None);
    };
    Ok(Some((exit_pool_in, exit_pool_out)))
}

pub fn process_on_exit_pool_trace(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    exit_pool_in: onExitPoolCall,
    exit_pool_out: onExitPoolReturn,
) -> Result<Option<Swap>> {
    let exit_kind: ExitKind = exit_pool_in
        .userData
        .get(0..32)
        .ok_or_eyre("JoinKind not found in userData")?
        .try_into()?;

    match exit_kind {
        ExitKind::ExactBptInForOneTokenOut => compute_exit_pool_exact_bpt_to_one_asset(
            state_by_sub_path,
            sub_trace_address,
            &exit_pool_in,
            &exit_pool_out,
        ),
        ExitKind::BptInForExactTokensOut => compute_exit_pool_bpt_to_exact_assets(
            state_by_sub_path,
            sub_trace_address,
            &exit_pool_in,
            &exit_pool_out,
        ),
        ExitKind::ExactBptInForAllTokensOut => {
            debug!("Skip exit pool to all token, no swap done");
            Ok(None)
        }
    }
}

fn compute_exit_pool_exact_bpt_to_one_asset(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    exit_pool_in: &onExitPoolCall,
    exit_pool_out: &onExitPoolReturn,
) -> Result<Option<Swap>> {
    let is_bpt_mint = false;
    let bpt_sent: U256 = U256::try_from_be_slice(
        exit_pool_in
            .userData
            .get(32..64)
            .ok_or_eyre("bpt amount sent not found in userData")?,
    )
    .ok_or_eyre("bpt amount sent cant be convert to U256")?;

    let (sdai_from_bpt, eure_from_bpt) = compute_sdai_eure_from_bpt(
        state_by_sub_path,
        sub_trace_address,
        bpt_sent,
        is_bpt_mint,
        &exit_pool_in.balances,
    )
    .wrap_err("Failed to compute the amount of sdai/eure from bpt ownership")?;

    let (sdai_received, eure_received) = (
        exit_pool_out
            ._0
            .get(SDAI_ARRAY_INDEX)
            .ok_or_eyre("sDAI output not found in on_exit_pool result")?,
        exit_pool_out
            ._0
            .get(EURE_ARRAY_INDEX)
            .ok_or_eyre("EURe output not found in on_exit_pool result")?,
    );

    match (sdai_received, eure_received) {
        (sdai_received, &U256::ZERO) => {
            let sdai_swapped_from_bpt = sdai_received.checked_sub(sdai_from_bpt).ok_or_eyre(
                "The amount of sDAI received is less than the amount of sDAI from BPT ownership",
            )?;

            Ok(Some(Swap {
                is_buy_eure: false,
                sdai_amount: sdai_swapped_from_bpt.to_string(),
                eure_amount: eure_from_bpt.to_string(),
            }))
        }
        (&U256::ZERO, eure_received) => {
            let eure_swapped_from_bpt = eure_received.checked_sub(eure_from_bpt).ok_or_eyre(
                "The amount of EURe received is less than the amount of EURe from BPT ownership",
            )?;

            Ok(Some(Swap {
                is_buy_eure: true,
                sdai_amount: sdai_from_bpt.to_string(),
                eure_amount: eure_swapped_from_bpt.to_string(),
            }))
        }
        _ => Err(eyre!("Unknown asset received")),
    }
}

fn compute_exit_pool_bpt_to_exact_assets(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    exit_pool_in: &onExitPoolCall,
    _: &onExitPoolReturn,
) -> Result<Option<Swap>> {
    let is_bpt_mint = false;
    let balance_sender_key = {
        let mut key = B256::left_padding_from(&exit_pool_in.sender.0.0).to_vec();
        key.extend_from_slice(&B256::ZERO.0);
        keccak256(key)
    };
    let bpt_owned_before: U256 = state_by_sub_path
        .get_load_value(&balance_sender_key, sub_trace_address, &Position::First)
        .ok_or_eyre("BPT owned before not found")?
        .into();
    let bpt_owned_after: U256 = state_by_sub_path
        .get_store_value(&balance_sender_key, sub_trace_address, &Position::First)
        .ok_or_eyre("BPT owned after not found")?
        .into();
    let bpt_burned = bpt_owned_before
        .checked_sub(bpt_owned_after)
        .ok_or_eyre("BPT owned increased after a onExitPool")?;

    let (sdai_from_bpt, eure_from_bpt) = compute_sdai_eure_from_bpt(
        state_by_sub_path,
        sub_trace_address,
        bpt_burned,
        is_bpt_mint,
        &exit_pool_in.balances,
    )
    .wrap_err("Failed to compute the amount of sdai/eure from bpt ownership")?;

    let sdai_received: U256 = U256::try_from_be_slice(
        exit_pool_in
            .userData
            .get(128..160)
            .ok_or_eyre("sdai received not found in userData")?,
    )
    .ok_or_eyre("sdai received cant be convert to U256")?;
    let eure_received: U256 = U256::try_from_be_slice(
        exit_pool_in
            .userData
            .get(160..192)
            .ok_or_eyre("eure received not found in userData")?,
    )
    .ok_or_eyre("eure received sent cant be convert to U256")?;

    match (sdai_received, eure_received) {
        (sdai_received, U256::ZERO) => {
            let sdai_swapped_from_bpt = sdai_received.checked_sub(sdai_from_bpt).ok_or_eyre(
                "BPT => sDAI, but sDAI received is less then the amount from BPT ownership",
            )?;

            Ok(Some(Swap {
                is_buy_eure: false,
                sdai_amount: sdai_swapped_from_bpt.to_string(),
                eure_amount: eure_from_bpt.to_string(),
            }))
        }
        (U256::ZERO, eure_received) => {
            let eure_swapped_from_bpt = eure_received.checked_sub(eure_from_bpt).ok_or_eyre(
                "BPT => EURe, but EURe received is less then the amount from BPT ownership",
            )?;

            Ok(Some(Swap {
                is_buy_eure: true,
                sdai_amount: sdai_from_bpt.to_string(),
                eure_amount: eure_swapped_from_bpt.to_string(),
            }))
        }
        (sdai_received, eure_received) if sdai_received >= sdai_from_bpt => {
            let sdai_swapped_from_bpt = sdai_received.checked_sub(sdai_from_bpt).ok_or_eyre(
                "BPT => +sDAI| -EURe, but sDAI received is less then the amount from BPT ownership",
            )?;
            let eure_swapped_from_bpt = eure_from_bpt.checked_sub(eure_received).ok_or_eyre(
                "BPT => +sDAI| -EURe, but EURe received is bigger then the amount from BPT ownership",
            )?;

            Ok(Some(Swap {
                is_buy_eure: false,
                sdai_amount: sdai_swapped_from_bpt.to_string(),
                eure_amount: eure_swapped_from_bpt.to_string(),
            }))
        }
        (sdai_received, eure_received) if sdai_received < sdai_from_bpt => {
            let sdai_swapped_from_bpt = sdai_from_bpt.checked_sub(sdai_received).ok_or_eyre(
                "BPT => -sDAI| +EURe, but sDAI received is bigger then the amount from BPT ownership",
            )?;
            let eure_swapped_from_bpt = eure_received.checked_sub(eure_from_bpt).ok_or_eyre(
                "BPT => -sDAI| +EURe, but EURe received is less then the amount from BPT ownership",
            )?;

            Ok(Some(Swap {
                is_buy_eure: true,
                sdai_amount: sdai_swapped_from_bpt.to_string(),
                eure_amount: eure_swapped_from_bpt.to_string(),
            }))
        }
        _ => Err(eyre!("Unknown assets received")),
    }
}
