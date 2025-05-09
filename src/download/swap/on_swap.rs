use crate::download::swap::{
    BALANCER_SDAI_EURE_POOL_ADDRESS, EURE_ADDRESS, EURE_ARRAY_INDEX, SDAI_ADDRESS,
    SDAI_ARRAY_INDEX, Swap, compute_sdai_eure_from_bpt,
};
use crate::helper::StateBySubPath;
use alloy::primitives::U256;
use alloy::rpc::types::trace::parity::{CallAction, TraceOutput};
use alloy::sol;
use alloy::sol_types::SolCall;
use eyre::{Context, OptionExt, Result, eyre};

sol!(
    #[derive(Debug, PartialEq, Eq)]
    enum SwapKind { GIVEN_IN, GIVEN_OUT }

    #[derive(Debug, PartialEq, Eq)]
    struct SwapRequest {
        SwapKind kind;
        address tokenIn;
        address tokenOut;
        uint256 amount;
        bytes32 poolId;
        uint256 lastChangeBlock;
        address from;
        address to;
        bytes userData;
    }

    #[derive(Debug, PartialEq, Eq)]
    function onSwap(SwapRequest memory swapRequest, uint256[] memory balances, uint256 indexIn, uint256 indexOut) internal virtual returns (uint256);
);

pub fn decode_in_out_on_swap(
    call_action: &CallAction,
    trace_output: &TraceOutput,
) -> Result<Option<(onSwapCall, U256)>> {
    let Ok(swap_in) = onSwapCall::abi_decode(&call_action.input) else {
        return Ok(None);
    };
    let Ok(swap_out) = onSwapCall::abi_decode_returns(trace_output.output()) else {
        return Ok(None);
    };
    Ok(Some((swap_in, swap_out)))
}

pub fn process_on_swap_trace(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    swap_in: onSwapCall,
    swap_out: U256,
) -> Result<Option<Swap>> {
    match (swap_in.swapRequest.tokenIn, swap_in.swapRequest.tokenOut) {
        (SDAI_ADDRESS, EURE_ADDRESS) => {
            return Ok(Some(compute_swap_csv_sdai_to_eure(&swap_in, swap_out)));
        }
        (EURE_ADDRESS, SDAI_ADDRESS) => {
            return Ok(Some(compute_swap_csv_eure_to_sdai(&swap_in, swap_out)));
        }
        _ => {}
    }

    match (swap_in.swapRequest.tokenIn, swap_in.swapRequest.tokenOut) {
        (BALANCER_SDAI_EURE_POOL_ADDRESS, EURE_ADDRESS) => {
            compute_swap_csv_bpt_to_eure(&state_by_sub_path, sub_trace_address, &swap_in, swap_out)
                .map(Some)
        }
        (BALANCER_SDAI_EURE_POOL_ADDRESS, SDAI_ADDRESS) => {
            compute_swap_csv_bpt_to_sdai(&state_by_sub_path, sub_trace_address, &swap_in, swap_out)
                .map(Some)
        }
        (EURE_ADDRESS, BALANCER_SDAI_EURE_POOL_ADDRESS) => {
            compute_swap_csv_eure_to_bpt(&state_by_sub_path, sub_trace_address, &swap_in, swap_out)
                .map(Some)
        }
        (SDAI_ADDRESS, BALANCER_SDAI_EURE_POOL_ADDRESS) => {
            compute_swap_csv_sdai_to_bpt(&state_by_sub_path, sub_trace_address, &swap_in, swap_out)
                .map(Some)
        }
        (SDAI_ADDRESS, SDAI_ADDRESS)
        | (EURE_ADDRESS, EURE_ADDRESS)
        | (BALANCER_SDAI_EURE_POOL_ADDRESS, BALANCER_SDAI_EURE_POOL_ADDRESS) => {
            Err(eyre!("onSwap same in and out"))
        }
        _ => Err(eyre::eyre!("onSwap unknown token")),
    }
}
fn compute_swap_csv_sdai_to_eure(swap_in: &onSwapCall, eure_received: U256) -> Swap {
    Swap {
        is_buy_eure: true,
        sdai_amount: swap_in.swapRequest.amount.to_string(),
        eure_amount: eure_received.to_string(),
    }
}
fn compute_swap_csv_eure_to_sdai(swap_in: &onSwapCall, sdai_received: U256) -> Swap {
    Swap {
        is_buy_eure: false,
        sdai_amount: sdai_received.to_string(),
        eure_amount: swap_in.swapRequest.amount.to_string(),
    }
}
fn compute_swap_csv_bpt_to_sdai(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    swap_in: &onSwapCall,
    sdai_received: U256,
) -> Result<Swap> {
    let is_bpt_mint = false;
    let (sdai_from_bpt, eure_from_bpt) = compute_sdai_eure_from_bpt(
        state_by_sub_path,
        sub_trace_address,
        swap_in.swapRequest.amount,
        is_bpt_mint,
        &swap_in.balances,
    )
    .wrap_err("Failed to compute the amount of sdai/eure from bpt ownership")?;

    let sdai_swapped_from_eure = sdai_received.checked_sub(sdai_from_bpt).ok_or_eyre(
        "The amount of sDAI received is less than the amount of sDAI from BPT ownership",
    )?;

    Ok(Swap {
        is_buy_eure: false,
        sdai_amount: sdai_swapped_from_eure.to_string(),
        eure_amount: eure_from_bpt.to_string(),
    })
}
fn compute_swap_csv_bpt_to_eure(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    swap_in: &onSwapCall,
    eure_received: U256,
) -> Result<Swap> {
    let is_bpt_mint = false;
    let (sdai_from_bpt, eure_from_bpt) = compute_sdai_eure_from_bpt(
        state_by_sub_path,
        sub_trace_address,
        swap_in.swapRequest.amount,
        is_bpt_mint,
        &swap_in.balances,
    )
    .wrap_err("Failed to compute the amount of sdai/eure from bpt ownership")?;

    let eure_swapped_from_sdai = eure_received.checked_sub(eure_from_bpt).ok_or_eyre(
        "The amount of EURe received is less than the amount of EURe from BPT ownership",
    )?;

    Ok(Swap {
        is_buy_eure: true,
        sdai_amount: sdai_from_bpt.to_string(),
        eure_amount: eure_swapped_from_sdai.to_string(),
    })
}
fn compute_swap_csv_sdai_to_bpt(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    swap_in: &onSwapCall,
    bpt_received: U256,
) -> Result<Swap> {
    let is_bpt_mint = true;

    let mut balances = swap_in.balances.clone();
    balances
        .get_mut(SDAI_ARRAY_INDEX)
        .ok_or_eyre("sDAI balance of the pool not found")?
        .checked_add(swap_in.swapRequest.amount)
        .ok_or_eyre("sDAI balance of the pool + sDAI swap amount overflow")?;

    let (sdai_from_bpt, eure_from_bpt) = compute_sdai_eure_from_bpt(
        state_by_sub_path,
        sub_trace_address,
        bpt_received,
        is_bpt_mint,
        &balances,
    )
    .wrap_err("Failed to compute the amount of sdai/eure from bpt ownership")?;

    let sdai_swapped_to_eure = swap_in
        .swapRequest
        .amount
        .checked_sub(sdai_from_bpt)
        .ok_or_eyre(
            "The amount of sDAI swapped is less than the amount of sDAI from BPT ownership",
        )?;

    Ok(Swap {
        is_buy_eure: true,
        sdai_amount: sdai_swapped_to_eure.to_string(),
        eure_amount: eure_from_bpt.to_string(),
    })
}
fn compute_swap_csv_eure_to_bpt(
    state_by_sub_path: &StateBySubPath,
    sub_trace_address: &[usize],
    swap_in: &onSwapCall,
    bpt_received: U256,
) -> Result<Swap> {
    let is_bpt_mint = true;

    let mut balances = swap_in.balances.clone();
    balances
        .get_mut(EURE_ARRAY_INDEX)
        .ok_or_eyre("EURe balance of the pool not found")?
        .checked_add(swap_in.swapRequest.amount)
        .ok_or_eyre("EURe balance of the pool + EURe swap amount overflow")?;

    let (sdai_from_bpt, eure_from_bpt) = compute_sdai_eure_from_bpt(
        state_by_sub_path,
        sub_trace_address,
        bpt_received,
        is_bpt_mint,
        &balances,
    )
    .wrap_err("Failed to compute the amount of sdai/eure from bpt ownership")?;

    let eure_swapped_to_sdai = swap_in
        .swapRequest
        .amount
        .checked_sub(eure_from_bpt)
        .ok_or_eyre(
            "The amount of EURe swapped is less than the amount of EURe from BPT ownership",
        )?;

    Ok(Swap {
        is_buy_eure: false,
        sdai_amount: sdai_from_bpt.to_string(),
        eure_amount: eure_swapped_to_sdai.to_string(),
    })
}
