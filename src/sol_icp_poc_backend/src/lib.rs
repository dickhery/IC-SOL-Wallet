// src/sol_icp_poc_backend/src/lib.rs

#[allow(deprecated)]
use ic_cdk::{export_candid, query, update};
use ic_cdk::api::canister_self as canister_id;
use ic_cdk::api::call::call_raw128;
use ic_cdk::api::caller;
use ic_cdk::trap;
use ic_cdk::management_canister::{
    SchnorrAlgorithm, SchnorrKeyId, SchnorrPublicKeyArgs, SchnorrPublicKeyResult,
    SignWithSchnorrArgs, SignWithSchnorrResult,
};
use ic_principal::Principal;
use ic_ledger_types::{
    AccountIdentifier, Memo, Subaccount, Timestamp, Tokens, TransferArgs, DEFAULT_FEE,
    MAINNET_LEDGER_CANISTER_ID,
};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    DefaultMemoryImpl, StableBTreeMap
};
use sha2::{Digest, Sha256, Sha224};
use std::cell::RefCell;
use bs58;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use lazy_static::lazy_static;
use base64::engine::general_purpose;
use base64::Engine as _;
use candid::{CandidType, Deserialize};
use hex;

const SOL_RPC_CANISTER: &str = "tghme-zyaaa-aaaar-qarca-cai";

lazy_static! {
    static ref SOL_RPC_PRINCIPAL: Principal = Principal::from_text(SOL_RPC_CANISTER).unwrap();
    static ref SERVICE_ACCOUNT: AccountIdentifier = AccountIdentifier::from_hex(
        "573292a9fdfff9ba7e23bcab9a99ab7db2a96c2e6697cf401a837f1c3a3280ed"
    ).unwrap();
    static ref KEY_ID: SchnorrKeyId = SchnorrKeyId {
        algorithm: SchnorrAlgorithm::Ed25519,
        name: "key_1".to_string()
    };
}

/* ----------------------------- SOL RPC TYPES ----------------------------- */

#[derive(CandidType, Deserialize, Clone)]
pub enum SolanaCluster { Mainnet }

#[derive(CandidType, Deserialize, Clone)]
pub enum RpcSources { Default(SolanaCluster) }

#[derive(CandidType, Deserialize, Clone)]
pub enum ConsensusStrategy { Equality }

#[derive(CandidType, Deserialize, Clone, Default)]
pub struct RpcConfig {
    #[serde(rename = "responseConsensus")]
    pub response_consensus: Option<ConsensusStrategy>,
    #[serde(rename = "responseSizeEstimate")]
    pub response_size_estimate: Option<u64>,
}

pub type Slot = u64;

#[derive(CandidType, Deserialize, Clone)]
pub enum CommitmentLevel {
    #[serde(rename = "finalized")]
    Finalized,
    #[serde(rename = "confirmed")]
    Confirmed,
    #[serde(rename = "processed")]
    Processed,
}

/* getBalance */
#[derive(CandidType, Deserialize, Clone)]
pub struct GetBalanceParams {
    pub pubkey: String,
    #[serde(rename = "minContextSlot")]
    pub min_context_slot: Option<Slot>,
    pub commitment: Option<CommitmentLevel>,
}

#[derive(CandidType, Deserialize, Clone)]
pub enum GetBalanceResult {
    Ok(u64),
    Err(String),
}

#[derive(CandidType, Deserialize, Clone)]
pub enum MultiGetBalanceResult {
    Consistent(GetBalanceResult),
    Inconsistent(Vec<(RpcSource, GetBalanceResult)>),
}

#[derive(CandidType, Deserialize, Clone)]
pub enum RpcSource {
    Supported(SupportedProvider),
    Custom(RpcEndpoint),
}
#[derive(CandidType, Deserialize, Clone)]
pub enum SupportedProvider {
    AnkrMainnet, AlchemyDevnet, DrpcMainnet, ChainstackDevnet, AlchemyMainnet,
    HeliusDevnet, AnkrDevnet, DrpcDevnet, ChainstackMainnet, PublicNodeMainnet, HeliusMainnet,
}
#[derive(CandidType, Deserialize, Clone)]
pub struct RpcEndpoint { pub url: String, pub headers: Option<Vec<HttpHeader>> }
#[derive(CandidType, Deserialize, Clone)]
pub struct HttpHeader { pub value: String, pub name: String }

/* getSlot */
#[derive(CandidType, Deserialize, Clone, Default)]
pub struct GetSlotRpcConfig {
    #[serde(rename = "roundingError")]
    pub rounding_error: Option<u64>,
    #[serde(rename = "responseConsensus")]
    pub response_consensus: Option<ConsensusStrategy>,
    #[serde(rename = "responseSizeEstimate")]
    pub response_size_estimate: Option<u64>,
}
#[derive(CandidType, Deserialize, Clone, Default)]
pub struct GetSlotParams {
    #[serde(rename = "minContextSlot")]
    pub min_context_slot: Option<Slot>,
    pub commitment: Option<CommitmentLevel>,
}
#[derive(CandidType, Deserialize, Clone)]
pub enum GetSlotResult {
    Ok(Slot),
    Err(String),
}
#[derive(CandidType, Deserialize, Clone)]
pub enum MultiGetSlotResult {
    Consistent(GetSlotResult),
    Inconsistent(Vec<(RpcSource, GetSlotResult)>),
}

/* getBlock */
#[derive(CandidType, Deserialize, Clone)]
pub enum TransactionDetails {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "signatures")]
    Signatures,
    #[serde(rename = "accounts")]
    Accounts,
}
#[derive(CandidType, Deserialize, Clone)]
pub enum GetBlockParamsCommitmentInner {
    #[serde(rename = "finalized")]
    Finalized,
    #[serde(rename = "confirmed")]
    Confirmed,
}
#[derive(CandidType, Deserialize, Clone)]
pub struct GetBlockParams {
    pub slot: Slot,
    #[serde(rename = "transactionDetails")]
    pub transaction_details: Option<TransactionDetails>,
    pub rewards: Option<bool>,
    pub commitment: Option<GetBlockParamsCommitmentInner>,
    #[serde(rename = "maxSupportedTransactionVersion")]
    pub max_supported_transaction_version: Option<u8>,
}
#[derive(CandidType, Deserialize, Clone)]
pub struct ConfirmedBlock { pub blockhash: String }
#[derive(CandidType, Deserialize, Clone)]
pub enum GetBlockResult {
    Ok(Option<ConfirmedBlock>),
    Err(String),
}
#[derive(CandidType, Deserialize, Clone)]
pub enum MultiGetBlockResult {
    Consistent(GetBlockResult),
    Inconsistent(Vec<(RpcSource, GetBlockResult)>),
}

/* sendTransaction */
#[derive(CandidType, Deserialize, Clone)]
pub enum SendTransactionEncoding {
    #[serde(rename = "base58")] Base58,
    #[serde(rename = "base64")] Base64,
}
#[derive(CandidType, Deserialize, Clone)]
pub struct SendTransactionParams {
    pub transaction: String,
    #[serde(rename = "skipPreflight")]
    pub skip_preflight: Option<bool>,
    pub encoding: Option<SendTransactionEncoding>,
    #[serde(rename = "preflightCommitment")]
    pub preflight_commitment: Option<CommitmentLevel>,
    #[serde(rename = "maxRetries")]
    pub max_retries: Option<u32>,
    #[serde(rename = "minContextSlot")]
    pub min_context_slot: Option<Slot>,
}
#[derive(CandidType, Deserialize, Clone)]
pub enum SendTransactionResult {
    Ok(String), // signature
    Err(String),
}
#[derive(CandidType, Deserialize, Clone)]
pub enum MultiSendTransactionResult {
    Consistent(SendTransactionResult),
    Inconsistent(Vec<(RpcSource, SendTransactionResult)>),
}
/* --------------------------- END SOL RPC TYPES --------------------------- */

type Memory = VirtualMemory<DefaultMemoryImpl>;
thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    static NONCE_MAP: RefCell<StableBTreeMap<String, u64, Memory>> =
        RefCell::new(StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0)))));

    // Ownership mappings (for optional linking)
    static OWNER_MAP: RefCell<StableBTreeMap<String, String, Memory>> =
        RefCell::new(StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))));
    static PRINCIPAL_MAP: RefCell<StableBTreeMap<String, String, Memory>> =
        RefCell::new(StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2)))));
}

const SERVICE_FEE: u64 = 10_000;     // 0.0001 ICP
const SERVICE_FEE_SOL: u64 = 20_000; // 0.0002 ICP

#[derive(Clone)]
struct CompiledInstrLike {
    prog_idx: u8,
    accts: Vec<u8>,
    data: Vec<u8>,
}

/* ------------------------------ helpers ------------------------------ */

fn derive_subaccount(sol_pubkey: &str) -> Subaccount {
    let mut hasher = Sha256::new();
    hasher.update(sol_pubkey.as_bytes());
    let hash = hasher.finalize();
    let mut subaccount = [0u8; 32];
    subaccount.copy_from_slice(&hash);
    Subaccount(subaccount)
}

fn verify_signature(sol_pubkey: &str, message: &[u8], signature: &[u8]) -> bool {
    let pubkey_bytes = match bs58::decode(sol_pubkey).into_vec() {
        Ok(bytes) if bytes.len() == 32 => bytes,
        _ => return false,
    };
    let pubkey_array: [u8; 32] = match pubkey_bytes.try_into() {
        Ok(arr) => arr,
        _ => return false,
    };
    let verifying_key = match VerifyingKey::from_bytes(&pubkey_array) {
        Ok(key) => key,
        _ => return false,
    };
    let sig_array: [u8; 64] = match signature.try_into() {
        Ok(arr) => arr,
        _ => return false,
    };
    let sig = Signature::from_bytes(&sig_array);
    verifying_key.verify(message, &sig).is_ok()
}

fn encode_compact(mut val: u64) -> Vec<u8> {
    let mut res = Vec::new();
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 { byte |= 0x80; }
        res.push(byte);
        if val == 0 { break; }
    }
    res
}

fn serialize_message(header: [u8; 3], accounts: &Vec<[u8; 32]>, blockhash: [u8; 32], instrs: &Vec<CompiledInstrLike>) -> Vec<u8> {
    let mut ser = Vec::new();
    ser.extend(header);
    ser.extend(encode_compact(accounts.len() as u64));
    for acc in accounts { ser.extend(*acc); }
    ser.extend(blockhash);
    ser.extend(encode_compact(instrs.len() as u64));
    for i in instrs {
        ser.push(i.prog_idx);
        ser.extend(encode_compact(i.accts.len() as u64));
        ser.extend(&i.accts);
        ser.extend(encode_compact(i.data.len() as u64));
        ser.extend(&i.data);
    }
    ser
}

/* ----------- dynamic cycles helper (shared) ----------- */

fn parse_required_cycles(err: &str) -> Option<u128> {
    if let Some(start) = err.find("but ") {
        let rest = &err[start + 4..];
        if let Some(end) = rest.find(" cycles") {
            let digits: String = rest[..end].chars().filter(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                if let Ok(v) = digits.parse::<u128>() {
                    return Some(v);
                }
            }
        }
    }
    if err.contains("TooFewCycles") {
        if let Some(start) = err.find("expected ") {
            let rest = &err[start + 9..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                if let Ok(v) = digits.parse::<u128>() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/* ----------- dynamic schnorr_public_key ----------- */

async fn schnorr_public_key_dynamic(args: &SchnorrPublicKeyArgs) -> Result<SchnorrPublicKeyResult, String> {
    let mgmt = Principal::management_canister();
    let arg_bytes = candid::encode_one(args).map_err(|e| format!("PK encode error: {:?}", e))?;

    let mut cycles: u128 = 10_000_000_000;

    for attempt in 0..3 {
        match call_raw128(mgmt, "schnorr_public_key", arg_bytes.clone(), cycles).await {
            Ok(raw) => {
                let reply: SchnorrPublicKeyResult =
                    candid::decode_one(&raw).map_err(|e| format!("PK decode error: {:?}", e))?;
                return Ok(reply);
            }
            Err((_code, msg)) => {
                if let Some(required) = parse_required_cycles(&msg) {
                    cycles = required.saturating_add(1_000_000_000);
                } else if attempt == 0 {
                    cycles = 30_000_000_000;
                } else {
                    return Err(format!("schnorr_public_key error: {}", msg));
                }
            }
        }
    }
    Err("schnorr_public_key error: retries exhausted".into())
}

/* ----------- dynamic sign_with_schnorr ----------- */

async fn sign_with_schnorr_dynamic(sign_args: &SignWithSchnorrArgs) -> Result<SignWithSchnorrResult, String> {
    let mgmt = Principal::management_canister();
    let arg_bytes = candid::encode_one(sign_args).map_err(|e| format!("Sign encode error: {:?}", e))?;

    let mut cycles: u128 = 30_000_000_000;

    for attempt in 0..3 {
        match call_raw128(mgmt, "sign_with_schnorr", arg_bytes.clone(), cycles).await {
            Ok(raw) => {
                let sig_reply: SignWithSchnorrResult = candid::decode_one(&raw)
                    .map_err(|e| format!("Sign decode error: {:?}", e))?;
                return Ok(sig_reply);
            }
            Err((_code, msg)) => {
                if let Some(required) = parse_required_cycles(&msg) {
                    cycles = required.saturating_add(1_000_000_000);
                } else if attempt == 0 {
                    cycles = 50_000_000_000;
                } else {
                    return Err(format!("Sign error: {}", msg));
                }
            }
        }
    }
    Err("Sign error: retries exhausted".into())
}

/* ----------- dynamic SOL RPC calls ----------- */

async fn call_sol_rpc_dynamic(method: &str, arg_bytes: Vec<u8>, mut cycles: u128) -> Result<Vec<u8>, String> {
    for attempt in 0..3 {
        match call_raw128(*SOL_RPC_PRINCIPAL, method, arg_bytes.clone(), cycles).await {
            Ok(raw) => return Ok(raw),
            Err((_code, msg)) => {
                if let Some(required) = parse_required_cycles(&msg) {
                    cycles = required.saturating_add(1_000_000_000);
                } else if attempt == 0 {
                    cycles = cycles.saturating_mul(2);
                } else {
                    return Err(format!("{} error: {}", method, msg));
                }
            }
        }
    }
    Err(format!("{} error: retries exhausted", method).into())
}

/* ----------- key derivation helpers now use dynamic PK ----------- */

async fn get_user_sol_pk_for_path(path_seed: Vec<u8>) -> [u8; 32] {
    let derivation_path: Vec<Vec<u8>> = vec![path_seed.clone()];
    let pk_args = SchnorrPublicKeyArgs {
        canister_id: None,
        derivation_path,
        key_id: KEY_ID.clone(),
    };
    let pk_res = schnorr_public_key_dynamic(&pk_args).await
        .unwrap_or_else(|e| trap(&format!("schnorr_public_key failed: {}", e)));
    let pk = pk_res.public_key;
    if pk.len() != 32 {
        ic_cdk::trap("Invalid public key length");
    }
    pk.try_into().unwrap()
}

async fn get_user_sol_pk_from_wallet(sol_pubkey: &str) -> [u8; 32] {
    let pubkey_bytes = match bs58::decode(sol_pubkey).into_vec() {
        Ok(bytes) if bytes.len() == 32 => bytes,
        _ => ic_cdk::trap("Invalid Solana pubkey"),
    };
    get_user_sol_pk_for_path(pubkey_bytes).await
}

async fn get_ii_sol_pk_for_caller() -> [u8; 32] {
    let principal = caller();
    let seed = principal.as_slice().to_vec(); // stable per-principal
    get_user_sol_pk_for_path(seed).await
}

/* ------------------------------ ownership / authorization ------------------------------ */

fn require_owner(sol_pubkey: &str) -> Result<(), String> {
    let caller_txt = caller().to_text();
    let owner = OWNER_MAP.with(|m| m.borrow().get(&sol_pubkey.to_string()));
    match owner {
        Some(o) if o == caller_txt => Ok(()),
        Some(_) => Err("Unauthorized: wallet linked to a different Internet Identity".into()),
        None => Err("Unauthorized: link this Solana wallet to your Internet Identity first".into())
    }
}

fn auth_by_phantom_or_owner(sol_pubkey: &str, message: &[u8], signature: &[u8]) -> Result<(), String> {
    if verify_signature(sol_pubkey, message, signature) {
        return Ok(());
    }
    require_owner(sol_pubkey)
}

/* -------------------------- nonce helpers (no self-call) -------------------------- */

fn read_or_init_nonce(key: &str) -> u64 {
    NONCE_MAP.with(|map| {
        let mut m = map.borrow_mut();
        let key_string = key.to_string();
        let cur = m.get(&key_string).unwrap_or(0);
        if cur == 0 {
            m.insert(key_string, 0);
        }
        cur
    })
}

/* ------------------------------ SOL RPC calls ------------------------------ */

async fn sol_get_balance_lamports(pubkey: String) -> Result<u64, String> {
    let rpc_sources = RpcSources::Default(SolanaCluster::Mainnet);
    let rpc_cfg: Option<RpcConfig> = None;

    let params = GetBalanceParams {
        pubkey,
        min_context_slot: None,
        commitment: Some(CommitmentLevel::Finalized),
    };

    let args = candid::encode_args((rpc_sources, rpc_cfg, params)).map_err(|e| e.to_string())?;

    let initial_cycles: u128 = 4_000_000_000;
    let raw = call_sol_rpc_dynamic("getBalance", args, initial_cycles).await?;

    let (multi,): (MultiGetBalanceResult,) =
        candid::decode_args(&raw).map_err(|e| format!("getBalance decode error: {:?}", e))?;

    match multi {
        MultiGetBalanceResult::Consistent(GetBalanceResult::Ok(lamports)) => Ok(lamports),
        MultiGetBalanceResult::Inconsistent(list) => {
            for (_src, r) in list {
                if let GetBalanceResult::Ok(l) = r { return Ok(l); }
            }
            Err("getBalance inconsistent and no Ok value".into())
        }
        MultiGetBalanceResult::Consistent(GetBalanceResult::Err(e)) => Err(format!("getBalance error: {e}")),
    }
}

async fn sol_get_finalized_slot() -> Result<u64, String> {
    let rpc_sources = RpcSources::Default(SolanaCluster::Mainnet);
    let cfg = Some(GetSlotRpcConfig {
        rounding_error: Some(50),
        ..Default::default()
    });
    let params = Some(GetSlotParams { min_context_slot: None, commitment: Some(CommitmentLevel::Finalized) });
    let args = candid::encode_args((rpc_sources, cfg, params)).map_err(|e| e.to_string())?;

    let initial_cycles: u128 = 4_000_000_000;
    let raw = call_sol_rpc_dynamic("getSlot", args, initial_cycles).await?;

    let (multi,): (MultiGetSlotResult,) = candid::decode_args(&raw).map_err(|e| format!("getSlot decode error: {:?}", e))?;
    match multi {
        MultiGetSlotResult::Consistent(GetSlotResult::Ok(slot)) => Ok(slot),
        MultiGetSlotResult::Inconsistent(list) => {
            for (_s, r) in list {
                if let GetSlotResult::Ok(slot) = r { return Ok(slot); }
            }
            Err("getSlot inconsistent and no Ok value".into())
        }
        MultiGetSlotResult::Consistent(GetSlotResult::Err(e)) => Err(format!("getSlot error: {e}")),
    }
}

async fn sol_get_blockhash_for_slot(slot: u64) -> Result<String, String> {
    let rpc_sources = RpcSources::Default(SolanaCluster::Mainnet);
    let rpc_cfg: Option<RpcConfig> = None;
    let params = GetBlockParams {
        slot,
        transaction_details: Some(TransactionDetails::None),
        rewards: Some(false),
        commitment: Some(GetBlockParamsCommitmentInner::Finalized),
        max_supported_transaction_version: None,
    };
    let args = candid::encode_args((rpc_sources, rpc_cfg, params)).map_err(|e| e.to_string())?;

    let initial_cycles: u128 = 4_000_000_000;
    let raw = call_sol_rpc_dynamic("getBlock", args, initial_cycles).await?;

    let (multi,): (MultiGetBlockResult,) = candid::decode_args(&raw).map_err(|e| format!("getBlock decode error: {:?}", e))?;
    match multi {
        MultiGetBlockResult::Consistent(GetBlockResult::Ok(Some(block))) => Ok(block.blockhash),
        MultiGetBlockResult::Inconsistent(list) => {
            for (_s, r) in list {
                if let GetBlockResult::Ok(Some(block)) = r { return Ok(block.blockhash); }
            }
            Err("getBlock inconsistent and no Ok value".into())
        }
        MultiGetBlockResult::Consistent(GetBlockResult::Ok(None)) =>
            Err("getBlock returned None (no block)".into()),
        MultiGetBlockResult::Consistent(GetBlockResult::Err(e)) =>
            Err(format!("getBlock error: {e}")),
    }
}

async fn sol_send_transaction_b64(tx_b64: String) -> Result<String, String> {
    let rpc_sources = RpcSources::Default(SolanaCluster::Mainnet);
    let rpc_cfg: Option<RpcConfig> = None;

    let params = SendTransactionParams {
        transaction: tx_b64,
        skip_preflight: Some(true),
        encoding: Some(SendTransactionEncoding::Base64),
        preflight_commitment: None,
        max_retries: None,
        min_context_slot: None,
    };

    let args = candid::encode_args((rpc_sources, rpc_cfg, params)).map_err(|e| e.to_string())?;

    let initial_cycles: u128 = 4_000_000_000;
    let raw = call_sol_rpc_dynamic("sendTransaction", args, initial_cycles).await?;

    let (multi,): (MultiSendTransactionResult,) = candid::decode_args(&raw).map_err(|e| format!("sendTransaction decode error: {:?}", e))?;
    match multi {
        MultiSendTransactionResult::Consistent(SendTransactionResult::Ok(sig)) => Ok(sig),
        MultiSendTransactionResult::Inconsistent(list) => {
            for (_s, r) in list {
                if let SendTransactionResult::Ok(sig) = r { return Ok(sig); }
            }
            Err("sendTransaction inconsistent and no Ok value".into())
        }
        MultiSendTransactionResult::Consistent(SendTransactionResult::Err(e)) =>
            Err(format!("sendTransaction error: {e}")),
    }
}

/* ------------------------------ public methods ------------------------------ */

#[query]
fn whoami() -> String {
    caller().to_text()
}

/* ---------- link/unlink ---------- */

#[update]
fn unlink_sol_pubkey() -> String {
    let principal_txt = caller().to_text();
    let linked = PRINCIPAL_MAP.with(|m| m.borrow().get(&principal_txt));
    match linked {
        Some(sol_pk) => {
            OWNER_MAP.with(|m| { m.borrow_mut().remove(&sol_pk); });
            PRINCIPAL_MAP.with(|m| { m.borrow_mut().remove(&principal_txt); });
            "Unlinked".into()
        }
        None => "No link found".into(),
    }
}

#[update]
fn link_sol_pubkey(sol_pubkey: String, signature: Vec<u8>) -> String {
    if sol_pubkey.is_empty() { return "Missing pubkey".into(); }
    let principal_txt = caller().to_text();
    let msg = format!("link {}", principal_txt);
    if !verify_signature(&sol_pubkey, msg.as_bytes(), &signature) {
        return "Invalid signature".into();
    }

    if let Some(owner) = OWNER_MAP.with(|m| m.borrow().get(&sol_pubkey)) {
        if owner == principal_txt {
            return "Already linked".into();
        } else {
            return "This Solana wallet is already linked to a different Internet Identity".into();
        }
    }

    if let Some(existing_pk) = PRINCIPAL_MAP.with(|m| m.borrow().get(&principal_txt)) {
        if existing_pk != sol_pubkey {
            return format!("This Internet Identity is already linked to {}", existing_pk);
        }
    }

    OWNER_MAP.with(|m| { m.borrow_mut().insert(sol_pubkey.clone(), principal_txt.clone()); });
    PRINCIPAL_MAP.with(|m| { m.borrow_mut().insert(principal_txt.clone(), sol_pubkey.clone()); });

    NONCE_MAP.with(|m| {
        if m.borrow().get(&sol_pubkey).is_none() {
            m.borrow_mut().insert(sol_pubkey.clone(), 0);
        }
    });

    "Linked".into()
}

/* ---------- II-only variants ---------- */

#[update]
async fn get_sol_deposit_address_ii() -> String {
    let user_pk = get_ii_sol_pk_for_caller().await;
    bs58::encode(user_pk).into_string()
}

#[update]
async fn get_deposit_address_ii() -> String {
    let sol_pk_str = bs58::encode(get_ii_sol_pk_for_caller().await).into_string();
    let subaccount = derive_subaccount(&sol_pk_str);
    let account = AccountIdentifier::new(&canister_id(), &subaccount);
    hex::encode(account.as_ref())
}

#[update]
async fn get_sol_balance_ii() -> u64 {
    let pubkey_str = bs58::encode(get_ii_sol_pk_for_caller().await).into_string();
    match sol_get_balance_lamports(pubkey_str).await {
        Ok(lamports) => lamports,
        Err(e) => {
            ic_cdk::println!("get_sol_balance_ii error: {}", e);
            0
        }
    }
}

#[update]
async fn get_balance_ii() -> u64 {
    let sol_pk_str = bs58::encode(get_ii_sol_pk_for_caller().await).into_string();
    let subaccount = derive_subaccount(&sol_pk_str);
    let account = AccountIdentifier::new(&canister_id(), &subaccount);
    let args = ic_ledger_types::AccountBalanceArgs { account };
    ic_ledger_types::account_balance(
        MAINNET_LEDGER_CANISTER_ID,
        &args,
    ).await.unwrap_or(Tokens::from_e8s(0)).e8s()
}

#[update]
async fn get_nonce_ii() -> u64 {
    let sol_pk_str = bs58::encode(get_ii_sol_pk_for_caller().await).into_string();
    read_or_init_nonce(&sol_pk_str)
}

#[update]
async fn transfer_ii(to: String, amount: u64) -> String {
    let sol_pk_str = bs58::encode(get_ii_sol_pk_for_caller().await).into_string();
    let current_nonce = read_or_init_nonce(&sol_pk_str);

    let subaccount = derive_subaccount(&sol_pk_str);
    let service_args = TransferArgs {
        memo: Memo(0),
        amount: Tokens::from_e8s(SERVICE_FEE),
        fee: DEFAULT_FEE,
        from_subaccount: Some(subaccount),
        to: *SERVICE_ACCOUNT,
        created_at_time: Some(Timestamp { timestamp_nanos: ic_cdk::api::time() }),
    };
    match ic_ledger_types::transfer(MAINNET_LEDGER_CANISTER_ID, &service_args).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return format!("Service fee transfer failed: {:?}", e),
        Err(e) => return format!("Call error for service fee: {:?}", e),
    }

    let to_account = match AccountIdentifier::from_hex(&to) {
        Ok(ai) => ai,
        Err(_) => return "Invalid to address".into(),
    };

    let transfer_args = TransferArgs {
        memo: Memo(0),
        amount: Tokens::from_e8s(amount),
        fee: DEFAULT_FEE,
        from_subaccount: Some(subaccount),
        to: to_account,
        created_at_time: Some(Timestamp { timestamp_nanos: ic_cdk::api::time() }),
    };
    let transfer_res = ic_ledger_types::transfer(MAINNET_LEDGER_CANISTER_ID, &transfer_args).await;
    match transfer_res {
        Ok(Ok(block_height)) => {
            NONCE_MAP.with(|map| {
                let mut map = map.borrow_mut();
                map.insert(sol_pk_str.clone(), current_nonce + 1);
            });
            let encoded_res: Result<(Vec<Vec<u8>>,), _> = ic_cdk::call(MAINNET_LEDGER_CANISTER_ID, "query_encoded_blocks", (block_height, 1u64)).await;
            match encoded_res {
                Ok((encoded,)) if encoded.len() == 1 => {
                    let mut hasher = Sha256::new();
                    hasher.update(&encoded[0]);
                    let hash_bytes = hasher.finalize();
                    let hash_hex = hex::encode(hash_bytes);
                    format!("Transfer successful: block {} hash {}", block_height, hash_hex)
                }
                // NOTE: no "failed" word in success path
                _ => format!("Transfer successful: block {} (hash not available yet)", block_height),
            }
        }
        Ok(Err(e)) => format!("Transfer failed: {:?}", e),
        Err(e) => format!("Call error: {:?}", e),
    }
}

#[update]
async fn transfer_sol_ii(to: String, amount: u64) -> String {
    let sol_pk_str = bs58::encode(get_ii_sol_pk_for_caller().await).into_string();
    let current_nonce = read_or_init_nonce(&sol_pk_str);

    let subaccount = derive_subaccount(&sol_pk_str);
    let service_args = TransferArgs {
        memo: Memo(0),
        amount: Tokens::from_e8s(SERVICE_FEE_SOL),
        fee: DEFAULT_FEE,
        from_subaccount: Some(subaccount),
        to: *SERVICE_ACCOUNT,
        created_at_time: Some(Timestamp { timestamp_nanos: ic_cdk::api::time() }),
    };
    match ic_ledger_types::transfer(MAINNET_LEDGER_CANISTER_ID, &service_args).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return format!("Service fee transfer failed: {:?}", e),
        Err(e) => return format!("Call error for service fee: {:?}", e),
    }

    let slot = match sol_get_finalized_slot().await {
        Ok(s) => s,
        Err(e) => return format!("Failed to get slot: {}", e),
    };
    let blockhash_b58 = match sol_get_blockhash_for_slot(slot).await {
        Ok(h) => h,
        Err(e) => return format!("Failed to get blockhash: {}", e),
    };
    let blockhash: [u8; 32] = match bs58::decode(&blockhash_b58).into_vec() {
        Ok(v) => match v.try_into() { Ok(a) => a, Err(_) => return "Invalid blockhash".into() },
        Err(_) => return "Invalid blockhash".into(),
    };

    let from_pk = get_ii_sol_pk_for_caller().await;
    let to_pk: [u8; 32] = match bs58::decode(&to).into_vec() {
        Ok(v) => match v.try_into() { Ok(a) => a, Err(_) => return "Invalid to address".into() },
        Err(_) => return "Invalid to address".into(),
    };
    let system_pk = [0u8; 32]; // system program
    let accounts = vec![from_pk, to_pk, system_pk];

    let header = [1u8, 0u8, 1u8];

    let mut data = Vec::new();
    data.extend(2u32.to_le_bytes()); // Transfer
    data.extend(amount.to_le_bytes());

    let instrs = vec![CompiledInstrLike { prog_idx: 2, accts: vec![0, 1], data }];

    let msg_ser = serialize_message(header, &accounts, blockhash, &instrs);

    let sign_args = SignWithSchnorrArgs {
        message: msg_ser.clone(),
        derivation_path: vec![caller().as_slice().to_vec()],
        key_id: KEY_ID.clone(),
        aux: None,
    };

    let sig_reply = match sign_with_schnorr_dynamic(&sign_args).await {
        Ok(s) => s,
        Err(e) => return e,
    };

    let mut tx_ser = encode_compact(1);
    tx_ser.extend(&sig_reply.signature);
    tx_ser.extend(&msg_ser);
    let tx_b64 = general_purpose::STANDARD.encode(tx_ser);

    let txid = match sol_send_transaction_b64(tx_b64).await {
        Ok(sig) => sig,
        Err(e) => return format!("Send failed: {}", e),
    };

    NONCE_MAP.with(|map| {
        let mut map = map.borrow_mut();
        map.insert(sol_pk_str.clone(), current_nonce + 1);
    });

    format!("Transfer successful: txid {}", txid)
}

/* ---------- Public (read) helpers ---------- */

#[update] // keep as update to avoid stale reads for apps
fn get_nonce(sol_pubkey: String) -> u64 {
    NONCE_MAP.with(|map| map.borrow().get(&sol_pubkey).unwrap_or(0))
}

#[query]
fn get_pid(sol_pubkey: String) -> String {
    let pubkey_bytes = match bs58::decode(sol_pubkey).into_vec() {
        Ok(bytes) if bytes.len() == 32 => bytes,
        _ => return "Invalid pubkey".to_string(),
    };
    let mut hasher = Sha224::new();
    hasher.update(&pubkey_bytes);
    let hash = hasher.finalize();
    let mut principal_bytes: Vec<u8> = hash.to_vec();
    principal_bytes.push(0x02); // Ed25519 type byte
    Principal::from_slice(&principal_bytes).to_text()
}

#[update]
async fn get_balance(sol_pubkey: String) -> u64 {
    let subaccount = derive_subaccount(&sol_pubkey);
    let account = AccountIdentifier::new(&canister_id(), &subaccount);
    let args = ic_ledger_types::AccountBalanceArgs { account };
    ic_ledger_types::account_balance(
        MAINNET_LEDGER_CANISTER_ID,
        &args,
    ).await.unwrap_or(Tokens::from_e8s(0)).e8s()
}

#[query]
fn get_deposit_address(sol_pubkey: String) -> String {
    let subaccount = derive_subaccount(&sol_pubkey);
    let account = AccountIdentifier::new(&canister_id(), &subaccount);
    hex::encode(account.as_ref())
}

#[update]
async fn get_sol_deposit_address(sol_pubkey: String) -> String {
    let user_pk = get_user_sol_pk_from_wallet(&sol_pubkey).await;
    bs58::encode(user_pk).into_string()
}

#[update]
async fn get_sol_balance(sol_pubkey: String) -> u64 {
    let user_pk = get_user_sol_pk_from_wallet(&sol_pubkey).await;
    let pubkey_str = bs58::encode(user_pk).into_string();
    match sol_get_balance_lamports(pubkey_str).await {
        Ok(lamports) => lamports,
        Err(e) => {
            ic_cdk::println!("get_sol_balance error: {}", e);
            0
        }
    }
}

/* ---------- Transfers (Phantom or II link) ---------- */

#[update]
async fn transfer(to: String, amount: u64, sol_pubkey: String, signature: Vec<u8>, nonce: u64) -> String {
    let current_nonce = get_nonce(sol_pubkey.clone());
    if nonce != current_nonce {
        return "Invalid nonce".to_string();
    }

    let message = format!("transfer to {} amount {} nonce {} service_fee {}", to, amount, nonce, SERVICE_FEE);
    if let Err(e) = auth_by_phantom_or_owner(&sol_pubkey, message.as_bytes(), &signature) {
        return e;
    }

    let subaccount = derive_subaccount(&sol_pubkey);

    let service_args = TransferArgs {
        memo: Memo(0),
        amount: Tokens::from_e8s(SERVICE_FEE),
        fee: DEFAULT_FEE,
        from_subaccount: Some(subaccount),
        to: *SERVICE_ACCOUNT,
        created_at_time: Some(Timestamp { timestamp_nanos: ic_cdk::api::time() }),
    };
    match ic_ledger_types::transfer(MAINNET_LEDGER_CANISTER_ID, &service_args).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return format!("Service fee transfer failed: {:?}", e),
        Err(e) => return format!("Call error for service fee: {:?}", e),
    }

    let to_account = match AccountIdentifier::from_hex(&to) {
        Ok(ai) => ai,
        Err(_) => return "Invalid to address".into(),
    };

    let transfer_args = TransferArgs {
        memo: Memo(0),
        amount: Tokens::from_e8s(amount),
        fee: DEFAULT_FEE,
        from_subaccount: Some(subaccount),
        to: to_account,
        created_at_time: Some(Timestamp { timestamp_nanos: ic_cdk::api::time() }),
    };
    let transfer_res = ic_ledger_types::transfer(MAINNET_LEDGER_CANISTER_ID, &transfer_args).await;
    match transfer_res {
        Ok(Ok(block_height)) => {
            NONCE_MAP.with(|map| {
                let mut map = map.borrow_mut();
                map.insert(sol_pubkey.clone(), current_nonce + 1);
            });
            let encoded_res: Result<(Vec<Vec<u8>>,), _> = ic_cdk::call(MAINNET_LEDGER_CANISTER_ID, "query_encoded_blocks", (block_height, 1u64)).await;
            match encoded_res {
                Ok((encoded,)) if encoded.len() == 1 => {
                    let mut hasher = Sha256::new();
                    hasher.update(&encoded[0]);
                    let hash_bytes = hasher.finalize();
                    let hash_hex = hex::encode(hash_bytes);
                    format!("Transfer successful: block {} hash {}", block_height, hash_hex)
                }
                _ => format!("Transfer successful: block {} (hash not available yet)", block_height),
            }
        }
        Ok(Err(e)) => format!("Transfer failed: {:?}", e),
        Err(e) => format!("Call error: {:?}", e),
    }
}

#[update]
async fn transfer_sol(to: String, amount: u64, sol_pubkey: String, signature: Vec<u8>, nonce: u64) -> String {
    let current_nonce = get_nonce(sol_pubkey.clone());
    if nonce != current_nonce {
        return "Invalid nonce".to_string();
    }

    let message = format!("transfer_sol to {} amount {} nonce {} service_fee {}", to, amount, nonce, SERVICE_FEE_SOL).into_bytes();
    if let Err(e) = auth_by_phantom_or_owner(&sol_pubkey, &message, &signature) {
        return e;
    }

    let subaccount = derive_subaccount(&sol_pubkey);

    let service_args = TransferArgs {
        memo: Memo(0),
        amount: Tokens::from_e8s(SERVICE_FEE_SOL),
        fee: DEFAULT_FEE,
        from_subaccount: Some(subaccount),
        to: *SERVICE_ACCOUNT,
        created_at_time: Some(Timestamp { timestamp_nanos: ic_cdk::api::time() }),
    };
    match ic_ledger_types::transfer(MAINNET_LEDGER_CANISTER_ID, &service_args).await {
        Ok(Ok(_)) => { /* proceed */ }
        Ok(Err(e)) => { return format!("Service fee transfer failed: {:?}", e); }
        Err(e)     => { return format!("Call error for service fee: {:?}", e); }
    }

    let slot = match sol_get_finalized_slot().await {
        Ok(s) => s,
        Err(e) => return format!("Failed to get slot: {}", e),
    };
    let blockhash_b58 = match sol_get_blockhash_for_slot(slot).await {
        Ok(h) => h,
        Err(e) => return format!("Failed to get blockhash: {}", e),
    };
    let blockhash: [u8; 32] = match bs58::decode(&blockhash_b58).into_vec() {
        Ok(v) => match v.try_into() { Ok(a) => a, Err(_) => return "Invalid blockhash".into() },
        Err(_) => return "Invalid blockhash".into(),
    };

    let from_pk = get_user_sol_pk_from_wallet(&sol_pubkey).await;
    let to_pk: [u8; 32] = match bs58::decode(&to).into_vec() {
        Ok(v) => match v.try_into() { Ok(a) => a, Err(_) => return "Invalid to address".into() },
        Err(_) => return "Invalid to address".into(),
    };
    let system_pk = [0u8; 32];
    let accounts = vec![from_pk, to_pk, system_pk];

    let header = [1u8, 0u8, 1u8];

    let mut data = Vec::new();
    data.extend(2u32.to_le_bytes());
    data.extend(amount.to_le_bytes());

    let instrs = vec![CompiledInstrLike { prog_idx: 2, accts: vec![0, 1], data }];

    let msg_ser = serialize_message(header, &accounts, blockhash, &instrs);

    let pubkey_bytes = match bs58::decode(&sol_pubkey).into_vec() {
        Ok(b) => b,
        Err(_) => return "Invalid user pubkey".into(),
    };
    let sign_args = SignWithSchnorrArgs {
        message: msg_ser.clone(),
        derivation_path: vec![pubkey_bytes],
        key_id: KEY_ID.clone(),
        aux: None,
    };

    let sig_reply = match sign_with_schnorr_dynamic(&sign_args).await {
        Ok(s) => s,
        Err(e) => return e,
    };

    let mut tx_ser = encode_compact(1);
    tx_ser.extend(&sig_reply.signature);
    tx_ser.extend(&msg_ser);
    let tx_b64 = general_purpose::STANDARD.encode(tx_ser);

    let txid = match sol_send_transaction_b64(tx_b64).await {
        Ok(sig) => sig,
        Err(e) => return format!("Send failed: {}", e),
    };

    NONCE_MAP.with(|map| {
        let mut map = map.borrow_mut();
        map.insert(sol_pubkey.clone(), current_nonce + 1);
    });

    format!("Transfer successful: txid {}", txid)
}

export_candid!();
