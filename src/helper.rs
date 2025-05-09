use crate::download::ProviderFiller;
use alloy::primitives::{B256, Bytes, TxHash, U256, b256};
use alloy::providers::ext::TraceApi;
use alloy::rpc::types::trace::parity::{VmInstruction, VmTrace};
use alloy::sol_types::private::u256;
use eyre::{OptionExt, Result};
use std::collections::{BTreeMap, HashMap};

pub trait DivUp
where
    Self: Sized,
{
    fn div_up(self, other: Self) -> Result<Self>;
}
impl DivUp for U256 {
    fn div_up(self, b: Self) -> Result<Self> {
        if self.is_zero() {
            return Ok(u256(0));
        }
        let one = u256(10).checked_pow(u256(18)).ok_or_eyre("10**18")?;
        self.checked_mul(one)
            .ok_or_eyre("div_up a * 10**18 overflow")?
            .checked_sub(u256(1))
            .ok_or_eyre("div_up (a / b) - 1 overflow")?
            .checked_div(b)
            .ok_or_eyre("div_up a / b overflow")?
            .checked_add(u256(1))
            .ok_or_eyre("div_up (((a * 10**18) - 1) / b) + 1 overflow")
    }
}

pub trait MulUp
where
    Self: Sized,
{
    fn mul_up(self, other: Self) -> Result<Self>;
}
impl MulUp for U256 {
    fn mul_up(self, b: Self) -> Result<Self> {
        if self.is_zero() || b.is_zero() {
            return Ok(u256(0));
        }
        let one = u256(10).checked_pow(u256(18)).ok_or_eyre("10**18")?;
        self.checked_mul(b)
            .ok_or_eyre("mul_up a * b overflow")?
            .checked_sub(u256(1))
            .ok_or_eyre("mul_up a * b = 0")?
            .checked_div(one)
            .ok_or_eyre("10**18 = 0")?
            .checked_add(u256(1))
            .ok_or_eyre("mul_up (((a * b) - 1) / 10**18) + 1 overflow")
    }
}

pub trait StringifyArrayUsize
where
    Self: Sized,
{
    fn stringify_vec_usize(self) -> String;
}
impl StringifyArrayUsize for &[usize] {
    fn stringify_vec_usize(self) -> String {
        self.iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join("-")
    }
}

pub async fn fetch_sub_vm_trace(
    provider: &ProviderFiller,
    tx_hash: TxHash,
    trace_address: &[usize],
) -> Result<VmTrace> {
    let vm_trace = provider
        .trace_replay_transaction(tx_hash)
        .vm_trace()
        .await?
        .vm_trace
        .ok_or_eyre(format!("Failed to fetch vm trace {:?}", tx_hash))?;

    extract_sub_vm_trace(vm_trace, trace_address)
}
pub fn extract_sub_vm_trace(mut vm_trace: VmTrace, trace_address: &[usize]) -> Result<VmTrace> {
    if trace_address.is_empty() {
        return Ok(vm_trace);
    }
    for mut sub_trace_to_take in trace_address.iter().cloned() {
        for (position, instruction) in vm_trace.ops.iter().enumerate() {
            let Some(sub) = instruction.sub.as_ref() else {
                continue;
            };

            if sub.ops.is_empty() && !is_console_static_call(&vm_trace, position)? {
                continue;
            }

            if sub_trace_to_take == 0 {
                vm_trace = sub.clone();
                break;
            }

            sub_trace_to_take -= 1;
        }

        if sub_trace_to_take != 0 {
            return Err(eyre::eyre!("trace_address is out of bounds"));
        }
    }

    Ok(vm_trace)
}

fn is_console_static_call(vm_trace: &VmTrace, static_call_position: usize) -> Result<bool> {
    let console_address = b256!("000000000000000000000000000000000000000000636F6e736F6c652e6c6f67");

    let vm_trace_before_static_call = vm_trace
        .ops
        .get(0..static_call_position)
        .ok_or_eyre("Failed to get instruction for check is console static call")?;

    for instruction in vm_trace_before_static_call.iter().rev() {
        if instruction.sub.is_some() {
            break;
        }
        let Some(ex) = instruction.ex.as_ref() else {
            continue;
        };
        if ex.push.iter().any(|p| p.to_be_bytes() == console_address) {
            return Ok(true);
        }
    }

    Ok(false)
}

#[derive(Debug)]
pub enum Position {
    First,
    Last,
    Id(usize),
}
#[derive(Debug, Default)]
pub struct StateBySubPath {
    // {store_key: { [sub_path]: [store_value] }}
    pub load_map: HashMap<B256, BTreeMap<Vec<usize>, Vec<B256>>>,
    pub store_map: HashMap<B256, BTreeMap<Vec<usize>, Vec<B256>>>,
}
impl StateBySubPath {
    pub fn new(vm_trace: &VmTrace) -> Self {
        let mut state_by_sub_path = StateBySubPath::default();
        state_by_sub_path.find_storage_value(vm_trace, &[]);
        state_by_sub_path
    }
    pub fn find_storage_value(&mut self, vm_trace: &VmTrace, sub_path: &[usize]) {
        let mut sub_path_counter = 0;

        for (instruction_position, instruction) in vm_trace.ops.iter().enumerate() {
            if let Some(next_instruction) = vm_trace.ops.get(instruction_position + 1) {
                if let Some((load_key, load_value)) =
                    Self::extract_storage_load(instruction, next_instruction, &vm_trace.code)
                {
                    Self::upsert_in_map(&mut self.load_map, &load_key, &load_value, sub_path);
                }
            }
            if let Some((store_key, store_value)) = Self::extract_storage_store(instruction) {
                Self::upsert_in_map(&mut self.store_map, &store_key, &store_value, sub_path);
            }

            if let Some(sub) = instruction.sub.as_ref() {
                if sub.ops.is_empty() {
                    continue;
                }
                self.find_storage_value(sub, &[sub_path, &[sub_path_counter]].concat());
                sub_path_counter += 1;
            }
        }
    }

    fn extract_storage_load(
        instruction: &VmInstruction,
        next_instruction: &VmInstruction,
        code: &Bytes,
    ) -> Option<(B256, B256)> {
        const SLOAD_OPCODE: u8 = 0x54;

        if code.get(next_instruction.pc)? != &SLOAD_OPCODE {
            return None;
        }
        let execution = instruction.ex.as_ref()?;
        let key = execution.push.last()?;

        let next_execution = next_instruction.ex.as_ref()?;
        let value = next_execution.push.last()?;

        Some((B256::from(*key), B256::from(*value)))
    }

    fn extract_storage_store(instruction: &VmInstruction) -> Option<(B256, B256)> {
        let execution = instruction.ex.as_ref()?;
        let storage = execution.store?;

        Some((B256::from(storage.key), B256::from(storage.val)))
    }

    fn upsert_in_map(
        storage_map: &mut HashMap<B256, BTreeMap<Vec<usize>, Vec<B256>>>,
        storage_key: &B256,
        storage_value: &B256,
        sub_path: &[usize],
    ) {
        let sub_path_map = match storage_map.get_mut(storage_key) {
            Some(m) => m,
            None => {
                storage_map.insert(*storage_key, BTreeMap::new());
                storage_map
                    .get_mut(storage_key)
                    .expect("storage_map.get_mut() failed just after insert")
            }
        };

        let sub_path_values = match sub_path_map.get_mut(sub_path) {
            Some(m) => m,
            None => {
                sub_path_map.insert(sub_path.to_vec(), Vec::new());
                sub_path_map
                    .get_mut(sub_path)
                    .expect("sub_path_map.get_mut() failed just after insert")
            }
        };

        sub_path_values.push(*storage_value);
    }

    pub fn get_load_value(
        &self,
        storage_key: &B256,
        sub_path: &[usize],
        position: &Position,
    ) -> Option<B256> {
        let values = self.load_map.get(storage_key)?.get(sub_path)?;

        match position {
            Position::First => values.first(),
            Position::Last => values.last(),
            Position::Id(id) => values.get(*id),
        }
        .cloned()
    }

    pub fn get_store_value(
        &self,
        storage_key: &B256,
        sub_path: &[usize],
        position: &Position,
    ) -> Option<B256> {
        let values = self.store_map.get(storage_key)?.get(sub_path)?;

        match position {
            Position::First => values.first(),
            Position::Last => values.last(),
            Position::Id(id) => values.get(*id),
        }
        .cloned()
    }
}

pub fn save_trace_to_file(
    mut vm_trace: VmTrace,
    tx_hash: &TxHash,
    prepend_name: &str,
) -> Result<()> {
    add_opcode_to_instruction(&mut vm_trace, &[]);
    std::fs::write(
        format!("{prepend_name}-{tx_hash}.json"),
        serde_json::to_string_pretty(&vm_trace)?,
    )?;

    Ok(())
}

fn add_opcode_to_instruction(vm_trace: &mut VmTrace, sub_path: &[usize]) {
    let mut sub_path_counter = 0;

    for instruction in vm_trace.ops.iter_mut() {
        instruction.op = vm_trace
            .code
            .get(instruction.pc)
            .map(|op| format!("{:#04x}", op));

        instruction.idx = Some(format!("{:?}", sub_path));

        if let Some(sub) = instruction.sub.as_mut() {
            if sub.ops.is_empty() {
                continue;
            }
            add_opcode_to_instruction(sub, &[sub_path, &[sub_path_counter]].concat());
            sub_path_counter += 1;
        }
    }
}
