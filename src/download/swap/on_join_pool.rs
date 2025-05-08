use crate::download::swap::{EURE_ARRAY_INDEX, SDAI_ARRAY_INDEX, Swap, compute_sdai_eure_from_bpt};
use crate::helper::{Position, StateBySubPath};
use alloy::primitives::{B256, U256, keccak256};
use alloy::rpc::types::trace::parity::{CallAction, TraceOutput};
use alloy::sol;
use alloy::sol_types::SolCall;
use eyre::{Context, OptionExt, Result, eyre};
use log::{debug, info};

sol!(
    #[derive(Debug, PartialEq, Eq)]
    function onJoinPool(bytes32 poolId, address sender, address recipient, uint256[] memory balances, uint256 lastChangeBlock, uint256 protocolSwapFeePercentage, bytes memory userData) external virtual returns (uint256[] memory, uint256[] memory);
);

enum JoinKind {
    Init,
    ExactTokensInForBptOut,
    TokenInForExactBptOut,
    AllTokensInForExactBptOut,
}
impl TryFrom<&[u8]> for JoinKind {
    type Error = eyre::Error;
    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        match value.last() {
            Some(0) => Ok(JoinKind::Init),
            Some(1) => Ok(JoinKind::ExactTokensInForBptOut),
            Some(2) => Ok(JoinKind::TokenInForExactBptOut),
            Some(3) => Ok(JoinKind::AllTokensInForExactBptOut),
            _ => Err(eyre!("Unknown join kind: {:?}", value)),
        }
    }
}
pub fn decode_in_out_on_join_pool(
    call_action: &CallAction,
    trace_output: &TraceOutput,
) -> Result<Option<(onJoinPoolCall, onJoinPoolReturn)>> {
    let Ok(join_pool_in) = onJoinPoolCall::abi_decode(&call_action.input) else {
        return Ok(None);
    };
    let Ok(join_pool_out) = onJoinPoolCall::abi_decode_returns(trace_output.output()) else {
        return Ok(None);
    };
    Ok(Some((join_pool_in, join_pool_out)))
}
pub fn process_on_join_pool_trace(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    join_pool_in: onJoinPoolCall,
    join_pool_out: onJoinPoolReturn,
) -> Result<Option<Swap>> {
    let join_kind: JoinKind = join_pool_in
        .userData
        .get(0..32)
        .ok_or_eyre("JoinKind not found in userData")?
        .try_into()?;
    if matches!(join_kind, JoinKind::Init) {
        info!("Skip the join init pool.");
        return Ok(None);
    }

    match join_kind {
        JoinKind::ExactTokensInForBptOut => compute_join_pool_exact_asset_to_bpt(
            state_by_sub_path,
            sub_trace_address,
            &join_pool_in,
            &join_pool_out,
        ),
        JoinKind::TokenInForExactBptOut => Err(eyre!("TokenInForExactBptOut not implemented yet")),
        JoinKind::AllTokensInForExactBptOut => {
            Err(eyre!("AllTokensInForExactBptOut not implemented yet"))
        }
        JoinKind::Init => Err(eyre!("Init join should already be handled")),
    }
}

fn compute_join_pool_exact_asset_to_bpt(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    join_pool_in: &onJoinPoolCall,
    join_pool_out: &onJoinPoolReturn,
) -> Result<Option<Swap>> {
    let is_bpt_mint = true;
    let sdai_sent = join_pool_out
        ._0
        .get(SDAI_ARRAY_INDEX)
        .ok_or_eyre("sDAI amount sent to the pool not found")?;
    let eure_sent = join_pool_out
        ._0
        .get(EURE_ARRAY_INDEX)
        .ok_or_eyre("EURe amount sent to the pool not found")?;
    let sdai_pool_balance = join_pool_in
        .balances
        .get(SDAI_ARRAY_INDEX)
        .ok_or_eyre("sDAI not found in pool balances")?
        .checked_add(*sdai_sent)
        .ok_or_eyre("Failed to add sDAI sent to the pool")?;
    let eure_pool_balance = join_pool_in
        .balances
        .get(EURE_ARRAY_INDEX)
        .ok_or_eyre("EURe not found in pool balances")?
        .checked_add(*sdai_sent)
        .ok_or_eyre("Failed to add EURe sent to the pool")?;
    let balance_recipient_key = {
        let mut key = B256::left_padding_from(&join_pool_in.recipient.0.0).to_vec();
        key.extend_from_slice(&B256::ZERO.0);
        keccak256(key)
    };

    let bpt_owned_before: U256 = state_by_sub_path
        .get_load_value(&balance_recipient_key, sub_trace_address, &Position::First)
        .ok_or_eyre("BPT owned before not found")?
        .into();
    let bpt_owned_after: U256 = state_by_sub_path
        .get_store_value(&balance_recipient_key, sub_trace_address, &Position::First)
        .ok_or_eyre("BPT owned after not found")?
        .into();
    let bpt_received = bpt_owned_after
        .checked_sub(bpt_owned_before)
        .ok_or_eyre("BPT owned decreased after a onJoinPool")?;

    let (sdai_from_bpt, eure_from_bpt) = compute_sdai_eure_from_bpt(
        state_by_sub_path,
        sub_trace_address,
        bpt_received,
        is_bpt_mint,
        &vec![sdai_pool_balance, eure_pool_balance],
    )
    .wrap_err("Failed to compute the amount of sdai/eure from bpt ownership")?;

    if sdai_sent > &sdai_from_bpt && eure_sent > &eure_from_bpt {
        debug!("Skip join pool, no swap done");
        return Ok(None);
    }
    match eure_from_bpt.cmp(eure_sent) {
        std::cmp::Ordering::Equal => {
            debug!("Skip join pool, no swap done");
            Ok(None)
        }
        std::cmp::Ordering::Greater => {
            // Our EURe from BPT is bigger than EURe we sent(so we bought EURe)
            let sdai_swap = sdai_sent
                .checked_sub(sdai_from_bpt)
                .ok_or_eyre("Buy EURe but our sDAI amount had increase")?;
            let eure_swap = eure_from_bpt
                .checked_sub(*eure_sent)
                .ok_or_eyre("Buy EURe but our EURe amount has decrease\n{:?}")?;

            Ok(Some(Swap {
                is_buy_eure: true,
                sdai_amount: sdai_swap.to_string(),
                eure_amount: eure_swap.to_string(),
                swap_fee_percentage: join_pool_in.protocolSwapFeePercentage.to_string(),
            }))
        }
        std::cmp::Ordering::Less => {
            // Our EURe from BPT is lesser than EURe we sent(so we sold EURe)
            let sdai_swap = sdai_from_bpt
                .checked_sub(*sdai_sent)
                .ok_or_eyre("Sell EURe but our sDAI amount had decrease")?;
            let eure_swap = eure_sent
                .checked_sub(eure_from_bpt)
                .ok_or_eyre("Sell EURe but our sDAI amount had increase")?;

            Ok(Some(Swap {
                is_buy_eure: false,
                sdai_amount: sdai_swap.to_string(),
                eure_amount: eure_swap.to_string(),
                swap_fee_percentage: join_pool_in.protocolSwapFeePercentage.to_string(),
            }))
        }
    }
}
