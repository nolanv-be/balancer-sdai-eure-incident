use crate::download::block_timestamp::TryIntoBlockTimestamp;
use crate::download::swap::{
    EURE_ARRAY_INDEX, SDAI_ARRAY_INDEX, Swap, SwapCsv, SwapFetcher, compute_sdai_eure_from_bpt,
};
use crate::helper::{StateBySubPath, fetch_sub_vm_trace};
use alloy::primitives::{TxHash, U256};
use alloy::rpc::types::trace::parity::{CallAction, LocalizedTransactionTrace};
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

impl SwapFetcher {
    pub async fn process_on_exit_pool_trace(
        &mut self,
        localized_trace: &LocalizedTransactionTrace,
        tx_hash: &TxHash,
        trace_path: &str,
        block_number: u64,
        call_action: &CallAction,
    ) -> Result<Option<SwapCsv>> {
        let Ok(exit_pool_in) = onExitPoolCall::abi_decode(&call_action.input) else {
            return Ok(None);
        };
        let Ok(exit_pool_out) = onExitPoolCall::abi_decode_returns(
            localized_trace
                .trace
                .result
                .as_ref()
                .ok_or_eyre("onJoinPool trace didn't have result")?
                .output(),
        ) else {
            return Ok(None);
        };

        let block_timestamp = block_number
            .try_into_block_timestamp(&mut self.block_timestamp_fetcher)
            .await?;

        let exit_kind: ExitKind = exit_pool_in
            .userData
            .get(0..32)
            .ok_or_eyre("JoinKind not found in userData")?
            .try_into()?;

        let (trace_address, sub_trace_address) = localized_trace
            .trace
            .trace_address
            .split_at(localized_trace.trace.trace_address.len() - 1);
        let vm_trace = fetch_sub_vm_trace(&self.provider, *tx_hash, trace_address).await?;

        let state_by_sub_path = StateBySubPath::new(&vm_trace);

        let Some(swap) = (match exit_kind {
            ExitKind::ExactBptInForOneTokenOut => compute_exit_pool_exact_bpt_to_one_asset(
                &state_by_sub_path,
                sub_trace_address,
                &exit_pool_in,
                &exit_pool_out,
            )?,
            ExitKind::BptInForExactTokensOut => {
                return Err(eyre!("BptInForExactTokensOut not implemented yet"));
            }
            ExitKind::ExactBptInForAllTokensOut => {
                debug!("Skip exit pool to all token, no swap done");
                return Ok(None);
            }
        }) else {
            return Ok(None);
        };

        Ok(Some(SwapCsv {
            is_buy_eure: swap.is_buy_eure,
            sdai_amount: swap.sdai_amount,
            eure_amount: swap.eure_amount,
            tx_hash: tx_hash.to_string(),
            block_number,
            block_timestamp,
            trace_path: trace_path.to_string(),
        }))
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
