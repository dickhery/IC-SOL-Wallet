// src/sol_icp_poc_backend/src/lib.rs

#[allow(deprecated)]
use ic_cdk::{export_candid, query, update};
use ic_cdk::api::canister_self as canister_id;
use ic_cdk::api::call::call_raw128;
use ic_cdk::trap;
use ic_cdk::management_canister::{
    ecdsa_public_key, http_request, sign_with_ecdsa, transform_context_from_query,
    EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgs, EcdsaPublicKeyResult,
    HttpHeader as CanisterHttpHeader, HttpMethod, HttpRequestArgs, HttpRequestResult,
    SchnorrAlgorithm, SchnorrKeyId, SchnorrPublicKeyArgs, SchnorrPublicKeyResult,
    SignWithEcdsaArgs, SignWithEcdsaResult, SignWithSchnorrArgs, SignWithSchnorrResult,
    TransformArgs,
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
use std::cmp::Ordering;
use bs58;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use lazy_static::lazy_static;
use base64::engine::general_purpose;
use base64::Engine as _;
use candid::{CandidType, Deserialize, Nat};
use ripemd::Ripemd160;
use serde_json::{json, Value};
use hex;

const SOL_RPC_CANISTER: &str = "tghme-zyaaa-aaaar-qarca-cai";
const SOLANA_HTTP_RPC_URL: &str = "https://api.mainnet-beta.solana.com";
const DOGE_PROVIDER_BASE_URL: &str = "https://api.blockcypher.com/v1/doge/main";
const DOGE_P2PKH_PREFIX: u8 = 0x1e;
const DOGE_P2SH_PREFIX: u8 = 0x16;
const SOL_TRANSFORM_BALANCE: &str = "sol_balance";
const DOGE_TRANSFORM_BALANCE: &str = "doge_balance";
const DOGE_TRANSFORM_TX_NEW: &str = "doge_tx_new";
const DOGE_TRANSFORM_TX_SEND: &str = "doge_tx_send";
const SECP256K1_ORDER: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
    0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b,
    0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
];
const SECP256K1_HALF_ORDER: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d,
    0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

fn threshold_key_name() -> &'static str {
    match option_env!("DFX_NETWORK") {
        Some("local") => "dfx_test_key",
        _ => "key_1",
    }
}

lazy_static! {
    static ref SOL_RPC_PRINCIPAL: Principal = Principal::from_text(SOL_RPC_CANISTER).unwrap();
    static ref SERVICE_ACCOUNT: AccountIdentifier = AccountIdentifier::from_hex(
        "573292a9fdfff9ba7e23bcab9a99ab7db2a96c2e6697cf401a837f1c3a3280ed"
    ).unwrap();
    static ref KEY_ID: SchnorrKeyId = SchnorrKeyId {
        algorithm: SchnorrAlgorithm::Ed25519,
        name: threshold_key_name().to_string()
    };
    static ref ECDSA_KEY_ID: EcdsaKeyId = EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: threshold_key_name().to_string(),
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
}

const SERVICE_FEE: u64 = 10_000;     // 0.0001 ICP
const SERVICE_FEE_SOL: u64 = 20_000; // 0.0002 ICP

#[derive(Clone)]
struct CompiledInstrLike {
    prog_idx: u8,
    accts: Vec<u8>,
    data: Vec<u8>,
}

#[derive(Clone)]
struct DogeKeyMaterial {
    path_seed: Vec<u8>,
    compressed_pubkey: Vec<u8>,
    address: String,
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

fn caller_principal() -> Principal {
    ic_cdk::api::msg_caller()
}

fn require_authenticated_caller() -> Principal {
    let caller = caller_principal();
    if caller == Principal::anonymous() {
        trap("Authentication required. Sign in with Internet Identity first.");
    }
    caller
}

async fn ii_wallet_seed_for_principal(caller: &Principal) -> String {
    bs58::encode(get_user_sol_pk_for_path(caller.as_slice().to_vec()).await).into_string()
}

async fn ii_sol_pubkey_for_principal(caller: &Principal) -> [u8; 32] {
    get_user_sol_pk_for_path(caller.as_slice().to_vec()).await
}

fn require_phantom_signature(sol_pubkey: &str, message: &[u8], signature: &[u8]) -> Result<(), String> {
    if verify_signature(sol_pubkey, message, signature) {
        Ok(())
    } else {
        Err("Invalid Phantom signature".into())
    }
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

fn double_sha256(bytes: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(bytes);
    let second = Sha256::digest(first);
    second.into()
}

fn base58check_encode(payload: &[u8]) -> String {
    let checksum = double_sha256(payload);
    let mut full = Vec::with_capacity(payload.len() + 4);
    full.extend_from_slice(payload);
    full.extend_from_slice(&checksum[..4]);
    bs58::encode(full).into_string()
}

fn decode_base58check(value: &str) -> Result<Vec<u8>, String> {
    let raw = bs58::decode(value)
        .into_vec()
        .map_err(|_| "Invalid Base58 value".to_string())?;

    if raw.len() < 5 {
        return Err("Invalid Base58Check payload".into());
    }

    let payload_len = raw.len() - 4;
    let (payload, checksum) = raw.split_at(payload_len);
    let expected = double_sha256(payload);
    if checksum != &expected[..4] {
        return Err("Invalid Base58Check checksum".into());
    }

    Ok(payload.to_vec())
}

fn validate_doge_address(address: &str) -> Result<(), String> {
    let payload = decode_base58check(address)?;
    if payload.len() != 21 {
        return Err("Invalid DOGE address length".into());
    }

    match payload[0] {
        DOGE_P2PKH_PREFIX | DOGE_P2SH_PREFIX => Ok(()),
        _ => Err("Unsupported DOGE address type".into()),
    }
}

fn doge_address_from_compressed_pubkey(compressed_pubkey: &[u8]) -> Result<String, String> {
    if compressed_pubkey.len() != 33 {
        return Err("Invalid compressed secp256k1 public key length".into());
    }

    let sha_hash = Sha256::digest(compressed_pubkey);
    let pubkey_hash = Ripemd160::digest(sha_hash);

    let mut payload = Vec::with_capacity(21);
    payload.push(DOGE_P2PKH_PREFIX);
    payload.extend_from_slice(&pubkey_hash);
    Ok(base58check_encode(&payload))
}

fn cmp_be(lhs: &[u8], rhs: &[u8]) -> Ordering {
    debug_assert_eq!(lhs.len(), rhs.len());
    for (l, r) in lhs.iter().zip(rhs.iter()) {
        match l.cmp(r) {
            Ordering::Equal => continue,
            non_eq => return non_eq,
        }
    }
    Ordering::Equal
}

fn sub_be(minuend: &[u8], subtrahend: &[u8]) -> Vec<u8> {
    debug_assert_eq!(minuend.len(), subtrahend.len());
    let mut out = vec![0u8; minuend.len()];
    let mut borrow = 0i16;

    for idx in (0..minuend.len()).rev() {
        let lhs = minuend[idx] as i16 - borrow;
        let rhs = subtrahend[idx] as i16;
        if lhs >= rhs {
            out[idx] = (lhs - rhs) as u8;
            borrow = 0;
        } else {
            out[idx] = (lhs + 256 - rhs) as u8;
            borrow = 1;
        }
    }

    out
}

fn der_encode_integer_component(component: &[u8]) -> Vec<u8> {
    let trimmed = component
        .iter()
        .position(|byte| *byte != 0)
        .map(|start| component[start..].to_vec())
        .unwrap_or_else(|| vec![0]);

    if trimmed[0] & 0x80 != 0 {
        let mut prefixed = Vec::with_capacity(trimmed.len() + 1);
        prefixed.push(0);
        prefixed.extend(trimmed);
        prefixed
    } else {
        trimmed
    }
}

fn der_encode_secp256k1_signature(compact_signature: &[u8]) -> Result<Vec<u8>, String> {
    if compact_signature.len() != 64 {
        return Err("Invalid secp256k1 signature length".into());
    }

    let mut normalized = [0u8; 64];
    normalized.copy_from_slice(compact_signature);

    if cmp_be(&normalized[32..], &SECP256K1_HALF_ORDER) == Ordering::Greater {
        let low_s = sub_be(&SECP256K1_ORDER, &normalized[32..]);
        normalized[32..].copy_from_slice(&low_s);
    }

    let r = der_encode_integer_component(&normalized[..32]);
    let s = der_encode_integer_component(&normalized[32..]);

    let mut der = Vec::with_capacity(r.len() + s.len() + 6);
    der.push(0x30);
    der.push((r.len() + s.len() + 4) as u8);
    der.push(0x02);
    der.push(r.len() as u8);
    der.extend(r);
    der.push(0x02);
    der.push(s.len() as u8);
    der.extend(s);
    Ok(der)
}

fn blockcypher_error_message(status: u16, body: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(errors) = value.get("errors").and_then(Value::as_array) {
            let joined = errors
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(s.clone()),
                    other => Some(other.to_string()),
                })
                .collect::<Vec<_>>()
                .join("; ");

            if !joined.is_empty() {
                return format!("DOGE provider error ({status}): {joined}");
            }
        }

        if let Some(message) = value.get("error").and_then(Value::as_str) {
            return format!("DOGE provider error ({status}): {message}");
        }
    }

    let fallback = String::from_utf8_lossy(body).trim().to_string();
    if fallback.is_empty() {
        format!("DOGE provider request failed with status {status}")
    } else {
        format!("DOGE provider error ({status}): {fallback}")
    }
}

fn json_string_array(value: &Value, field: &str) -> Result<Vec<String>, String> {
    let arr = value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("DOGE provider response missing `{field}`"))?;

    arr.iter()
        .map(|entry| {
            entry
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("DOGE provider response has non-string `{field}` entry"))
        })
        .collect()
}

fn solana_error_message(status: u16, body: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(|err| err.get("message"))
            .and_then(Value::as_str)
        {
            return format!("Solana RPC error ({status}): {message}");
        }
    }

    let fallback = String::from_utf8_lossy(body).trim().to_string();
    if fallback.is_empty() {
        format!("Solana RPC request failed with status {status}")
    } else {
        format!("Solana RPC error ({status}): {fallback}")
    }
}

fn canonical_solana_error_body(status: u16, body: &[u8]) -> Vec<u8> {
    let message = solana_error_message(status, body);
    serde_json::to_vec(&json!({ "error": message }))
        .unwrap_or_else(|_| br#"{"error":"Solana RPC request failed"}"#.to_vec())
}

fn canonicalize_solana_balance(body: &[u8]) -> Result<Vec<u8>, String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| format!("Failed to parse Solana balance JSON: {e}"))?;

    let lamports = value
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(Value::as_u64)
        .ok_or_else(|| "Solana RPC response missing `result.value`".to_string())?;

    serde_json::to_vec(&json!({ "value": lamports }))
        .map_err(|e| format!("Failed to encode canonical Solana balance response: {e}"))
}

fn canonical_blockcypher_error_body(status: u16, body: &[u8]) -> Vec<u8> {
    let mut errors = Vec::new();

    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(arr) = value.get("errors").and_then(Value::as_array) {
            for item in arr {
                match item {
                    Value::String(s) => errors.push(s.clone()),
                    Value::Object(obj) => {
                        if let Some(message) = obj.get("error").and_then(Value::as_str) {
                            errors.push(message.to_string());
                        } else {
                            errors.push(item.to_string());
                        }
                    }
                    other => errors.push(other.to_string()),
                }
            }
        }

        if let Some(message) = value.get("error").and_then(Value::as_str) {
            errors.push(message.to_string());
        }
    }

    if errors.is_empty() {
        errors.push(format!("DOGE provider request failed with status {status}"));
    }

    errors.sort();
    errors.dedup();

    serde_json::to_vec(&json!({ "errors": errors }))
        .unwrap_or_else(|_| br#"{"errors":["DOGE provider request failed"]}"#.to_vec())
}

fn canonicalize_blockcypher_tx_new(body: &[u8]) -> Result<Vec<u8>, String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| format!("Failed to parse DOGE tx skeleton JSON: {e}"))?;

    let tosign = json_string_array(&value, "tosign")?;
    let tx_obj = value
        .get("tx")
        .and_then(Value::as_object)
        .ok_or_else(|| "DOGE provider response missing `tx`".to_string())?;

    let mut tx = tx_obj.clone();
    for key in [
        "hash",
        "size",
        "block_height",
        "block_index",
        "confirmations",
        "double_spend",
        "received",
        "relayed_by",
    ] {
        tx.remove(key);
    }

    serde_json::to_vec(&json!({
        "tx": Value::Object(tx),
        "tosign": tosign,
        "signatures": [],
        "pubkeys": [],
    }))
    .map_err(|e| format!("Failed to encode canonical DOGE tx skeleton: {e}"))
}

fn canonicalize_blockcypher_tx_send(body: &[u8]) -> Result<Vec<u8>, String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| format!("Failed to parse DOGE tx send JSON: {e}"))?;

    let hash = value
        .get("tx")
        .and_then(|tx| tx.get("hash"))
        .and_then(Value::as_str)
        .ok_or_else(|| "DOGE provider response missing broadcast transaction hash".to_string())?;

    serde_json::to_vec(&json!({
        "tx": { "hash": hash }
    }))
    .map_err(|e| format!("Failed to encode canonical DOGE send response: {e}"))
}

fn canonicalize_blockcypher_tx_hash(hash: &str) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&json!({
        "tx": { "hash": hash }
    }))
    .map_err(|e| format!("Failed to encode canonical DOGE send response: {e}"))
}

fn duplicate_tx_hash_from_text(text: &str) -> Option<String> {
    let marker = "Transaction with hash ";
    let start = text.find(marker)? + marker.len();
    let candidate: String = text[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .collect();

    if candidate.len() == 64 {
        Some(candidate)
    } else {
        None
    }
}

fn duplicate_tx_hash_from_blockcypher_error_body(body: &[u8]) -> Option<String> {
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(arr) = value.get("errors").and_then(Value::as_array) {
            for item in arr {
                let text = match item {
                    Value::String(s) => s.as_str(),
                    Value::Object(obj) => obj.get("error").and_then(Value::as_str).unwrap_or(""),
                    _ => "",
                };

                if text.to_ascii_lowercase().contains("already exists") {
                    if let Some(hash) = duplicate_tx_hash_from_text(text) {
                        return Some(hash);
                    }
                }
            }
        }

        if let Some(message) = value.get("error").and_then(Value::as_str) {
            if message.to_ascii_lowercase().contains("already exists") {
                if let Some(hash) = duplicate_tx_hash_from_text(message) {
                    return Some(hash);
                }
            }
        }
    }

    let fallback = String::from_utf8_lossy(body);
    if fallback.to_ascii_lowercase().contains("already exists") {
        return duplicate_tx_hash_from_text(&fallback);
    }

    None
}

fn canonicalize_blockcypher_balance(body: &[u8]) -> Result<Vec<u8>, String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| format!("Failed to parse DOGE balance JSON: {e}"))?;

    let final_balance = value
        .get("final_balance")
        .and_then(Value::as_u64)
        .ok_or_else(|| "DOGE provider response missing `final_balance`".to_string())?;

    serde_json::to_vec(&json!({
        "final_balance": final_balance
    }))
    .map_err(|e| format!("Failed to encode canonical DOGE balance response: {e}"))
}

fn canonicalize_blockcypher_success(context: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    match context {
        DOGE_TRANSFORM_BALANCE => canonicalize_blockcypher_balance(body),
        DOGE_TRANSFORM_TX_NEW => canonicalize_blockcypher_tx_new(body),
        DOGE_TRANSFORM_TX_SEND => canonicalize_blockcypher_tx_send(body),
        _ => Err(format!("Unknown DOGE transform context: {context}")),
    }
}

async fn doge_http_json(
    method: HttpMethod,
    path: &str,
    body: Option<Vec<u8>>,
    max_response_bytes: u64,
    transform_context: &str,
) -> Result<Value, String> {
    let mut headers = vec![
        CanisterHttpHeader {
            name: "Accept".into(),
            value: "application/json".into(),
        },
        CanisterHttpHeader {
            name: "User-Agent".into(),
            value: "ic-sol-wallet-doge/1.0".into(),
        },
    ];

    if body.is_some() {
        headers.push(CanisterHttpHeader {
            name: "Content-Type".into(),
            value: "application/json".into(),
        });

        if transform_context == DOGE_TRANSFORM_TX_SEND {
            let idempotency_key = body
                .as_ref()
                .map(|bytes| hex::encode(Sha256::digest(bytes)))
                .unwrap_or_default();

            headers.push(CanisterHttpHeader {
                name: "Idempotency-Key".into(),
                value: format!("doge-send-{idempotency_key}"),
            });
        }
    }

    let request = HttpRequestArgs {
        url: format!("{}{}", DOGE_PROVIDER_BASE_URL, path),
        max_response_bytes: Some(max_response_bytes),
        method,
        headers,
        body,
        transform: Some(transform_context_from_query(
            "transform_blockcypher_response".to_string(),
            transform_context.as_bytes().to_vec(),
        )),
    };

    let response = http_request(&request)
        .await
        .map_err(|e| format!("DOGE HTTPS outcall failed: {e}"))?;

    let status = response.status.to_string().parse::<u16>().unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(blockcypher_error_message(status, &response.body));
    }

    serde_json::from_slice(&response.body)
        .map_err(|e| format!("Failed to decode DOGE provider JSON: {e}"))
}

async fn sol_get_balance_http_lamports(pubkey: &str) -> Result<u64, String> {
    let request_body = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBalance",
        "params": [pubkey, { "commitment": "finalized" }],
    }))
    .map_err(|e| format!("Failed to encode Solana RPC request: {e}"))?;

    let request = HttpRequestArgs {
        url: SOLANA_HTTP_RPC_URL.to_string(),
        max_response_bytes: Some(4_096),
        method: HttpMethod::POST,
        headers: vec![
            CanisterHttpHeader {
                name: "Accept".into(),
                value: "application/json".into(),
            },
            CanisterHttpHeader {
                name: "Content-Type".into(),
                value: "application/json".into(),
            },
            CanisterHttpHeader {
                name: "User-Agent".into(),
                value: "ic-sol-wallet-sol/1.0".into(),
            },
            CanisterHttpHeader {
                name: "Host".into(),
                value: "api.mainnet-beta.solana.com".into(),
            },
        ],
        body: Some(request_body),
        transform: Some(transform_context_from_query(
            "transform_solana_rpc_response".to_string(),
            SOL_TRANSFORM_BALANCE.as_bytes().to_vec(),
        )),
    };

    let response = http_request(&request)
        .await
        .map_err(|e| format!("SOL HTTPS outcall failed: {e}"))?;

    let status = response.status.to_string().parse::<u16>().unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(solana_error_message(status, &response.body));
    }

    let body: Value = serde_json::from_slice(&response.body)
        .map_err(|e| format!("Failed to decode Solana RPC JSON: {e}"))?;

    body.get("value")
        .and_then(Value::as_u64)
        .ok_or_else(|| "Solana RPC canonical response missing `value`".into())
}

async fn doge_get_balance_satoshis(address: &str) -> Result<u64, String> {
    validate_doge_address(address)?;
    let body = doge_http_json(
        HttpMethod::GET,
        &format!("/addrs/{address}/balance"),
        None,
        4_096,
        DOGE_TRANSFORM_BALANCE,
    )
    .await?;

    body.get("final_balance")
        .and_then(Value::as_u64)
        .ok_or_else(|| "DOGE provider response missing `final_balance`".into())
}

async fn doge_create_tx_skeleton(from_address: &str, to_address: &str, amount: u64) -> Result<Value, String> {
    validate_doge_address(from_address)?;
    validate_doge_address(to_address)?;
    if amount == 0 {
        return Err("DOGE amount must be greater than zero".into());
    }

    let request = json!({
        "inputs": [{ "addresses": [from_address] }],
        "outputs": [{ "addresses": [to_address], "value": amount }],
        "confirmations": 1,
        "preference": "medium"
    });

    let request_body = serde_json::to_vec(&request)
        .map_err(|e| format!("Failed to encode DOGE transaction request: {e}"))?;

    doge_http_json(
        HttpMethod::POST,
        "/txs/new",
        Some(request_body),
        32_768,
        DOGE_TRANSFORM_TX_NEW,
    )
    .await
}

async fn ecdsa_key_material_for_path(path_seed: Vec<u8>) -> Result<DogeKeyMaterial, String> {
    let args = EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: vec![path_seed.clone()],
        key_id: ECDSA_KEY_ID.clone(),
    };

    let reply: EcdsaPublicKeyResult = ecdsa_public_key(&args)
        .await
        .map_err(|e| format!("Failed to derive DOGE public key: {e}"))?;

    let address = doge_address_from_compressed_pubkey(&reply.public_key)?;
    Ok(DogeKeyMaterial {
        path_seed,
        compressed_pubkey: reply.public_key,
        address,
    })
}

async fn ii_doge_key_material(caller: &Principal) -> Result<DogeKeyMaterial, String> {
    ecdsa_key_material_for_path(caller.as_slice().to_vec()).await
}

async fn doge_key_material_from_wallet(sol_pubkey: &str) -> Result<DogeKeyMaterial, String> {
    let path_seed = bs58::decode(sol_pubkey)
        .into_vec()
        .map_err(|_| "Invalid Solana pubkey".to_string())?;

    if path_seed.len() != 32 {
        return Err("Invalid Solana pubkey".into());
    }

    ecdsa_key_material_for_path(path_seed).await
}

async fn doge_sign_tosign_hashes(path_seed: &[u8], tosign: &[String]) -> Result<Vec<String>, String> {
    let mut signatures = Vec::with_capacity(tosign.len());

    for hash_hex in tosign {
        let hash = hex::decode(hash_hex).map_err(|_| "DOGE provider returned invalid hex to sign".to_string())?;
        if hash.len() != 32 {
            return Err("DOGE provider returned a non-32-byte signing payload".into());
        }

        let args = SignWithEcdsaArgs {
            message_hash: hash,
            derivation_path: vec![path_seed.to_vec()],
            key_id: ECDSA_KEY_ID.clone(),
        };

        let reply: SignWithEcdsaResult = sign_with_ecdsa(&args)
            .await
            .map_err(|e| format!("Failed to sign DOGE transaction: {e}"))?;

        let der_signature = der_encode_secp256k1_signature(&reply.signature)?;
        signatures.push(hex::encode(der_signature));
    }

    Ok(signatures)
}

async fn doge_transfer_with_material(material: &DogeKeyMaterial, to_address: &str, amount: u64) -> Result<String, String> {
    let mut skeleton = doge_create_tx_skeleton(&material.address, to_address, amount).await?;
    let tosign = json_string_array(&skeleton, "tosign")?;
    if tosign.is_empty() {
        return Err("DOGE provider did not return any inputs to sign".into());
    }

    let signatures = doge_sign_tosign_hashes(&material.path_seed, &tosign).await?;
    let pubkey_hex = hex::encode(&material.compressed_pubkey);

    skeleton["signatures"] = json!(signatures);
    skeleton["pubkeys"] = json!(vec![pubkey_hex; tosign.len()]);

    let request_body = serde_json::to_vec(&skeleton)
        .map_err(|e| format!("Failed to encode DOGE signed transaction: {e}"))?;

    let sent = doge_http_json(
        HttpMethod::POST,
        "/txs/send",
        Some(request_body),
        32_768,
        DOGE_TRANSFORM_TX_SEND,
    )
    .await?;
    sent.get("tx")
        .and_then(|tx| tx.get("hash"))
        .and_then(Value::as_str)
        .map(|hash| hash.to_string())
        .ok_or_else(|| "DOGE provider response missing broadcast transaction hash".into())
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
    match sol_get_balance_http_lamports(&pubkey).await {
        Ok(lamports) => return Ok(lamports),
        Err(err) => ic_cdk::println!("sol_get_balance_lamports HTTP path failed, falling back to Sol RPC canister: {}", err),
    }

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
    caller_principal().to_text()
}

/* ---------- link/unlink ---------- */

#[update]
fn unlink_sol_pubkey() -> String {
    "Wallet linking is no longer supported. Use Internet Identity or Phantom directly.".into()
}

#[update]
fn link_sol_pubkey(_sol_pubkey: String, _signature: Vec<u8>) -> String {
    "Wallet linking is no longer supported. Use Internet Identity or Phantom directly.".into()
}

#[query]
fn transform_blockcypher_response(args: TransformArgs) -> HttpRequestResult {
    let context = String::from_utf8(args.context).unwrap_or_default();
    let status = args.response.status.to_string().parse::<u16>().unwrap_or(500);

    let duplicate_send_hash = if context == DOGE_TRANSFORM_TX_SEND {
        duplicate_tx_hash_from_blockcypher_error_body(&args.response.body)
    } else {
        None
    };

    let (canonical_status, canonical_body) = if let Some(hash) = duplicate_send_hash {
        match canonicalize_blockcypher_tx_hash(&hash) {
            Ok(body) => (200u16, body),
            Err(e) => (
                500u16,
                serde_json::to_vec(&json!({ "errors": [e] }))
                    .unwrap_or_else(|_| br#"{"errors":["DOGE transform failed"]}"#.to_vec()),
            ),
        }
    } else if (200..300).contains(&status) {
        match canonicalize_blockcypher_success(&context, &args.response.body) {
            Ok(body) => (200u16, body),
            Err(e) => (
                500u16,
                serde_json::to_vec(&json!({ "errors": [e] }))
                    .unwrap_or_else(|_| br#"{"errors":["DOGE transform failed"]}"#.to_vec()),
            ),
        }
    } else {
        (status, canonical_blockcypher_error_body(status, &args.response.body))
    };

    HttpRequestResult {
        status: Nat::from(canonical_status),
        headers: vec![],
        body: canonical_body,
    }
}

#[query]
fn transform_solana_rpc_response(args: TransformArgs) -> HttpRequestResult {
    let context = String::from_utf8(args.context).unwrap_or_default();
    let status = args.response.status.to_string().parse::<u16>().unwrap_or(500);

    let (canonical_status, canonical_body) = if (200..300).contains(&status) {
        match context.as_str() {
            SOL_TRANSFORM_BALANCE => match canonicalize_solana_balance(&args.response.body) {
                Ok(body) => (200u16, body),
                Err(e) => (
                    500u16,
                    serde_json::to_vec(&json!({ "error": e }))
                        .unwrap_or_else(|_| br#"{"error":"Solana transform failed"}"#.to_vec()),
                ),
            },
            _ => (
                500u16,
                serde_json::to_vec(&json!({ "error": format!("Unknown Solana transform context: {context}") }))
                    .unwrap_or_else(|_| br#"{"error":"Unknown Solana transform context"}"#.to_vec()),
            ),
        }
    } else {
        (status, canonical_solana_error_body(status, &args.response.body))
    };

    HttpRequestResult {
        status: Nat::from(canonical_status),
        headers: vec![],
        body: canonical_body,
    }
}

/* ---------- II-only variants ---------- */

#[update]
async fn get_doge_deposit_address_ii() -> String {
    let caller = require_authenticated_caller();
    ii_doge_key_material(&caller)
        .await
        .unwrap_or_else(|e| trap(&format!("Failed to derive DOGE address: {e}")))
        .address
}

#[update]
async fn get_doge_balance_ii() -> u64 {
    let caller = require_authenticated_caller();
    let material = match ii_doge_key_material(&caller).await {
        Ok(material) => material,
        Err(e) => {
            ic_cdk::println!("get_doge_balance_ii derive error: {}", e);
            return 0;
        }
    };

    match doge_get_balance_satoshis(&material.address).await {
        Ok(balance) => balance,
        Err(e) => {
            ic_cdk::println!("get_doge_balance_ii error: {}", e);
            0
        }
    }
}

#[update]
async fn get_sol_deposit_address_ii() -> String {
    let caller = require_authenticated_caller();
    let user_pk = ii_sol_pubkey_for_principal(&caller).await;
    bs58::encode(user_pk).into_string()
}

#[update]
async fn get_deposit_address_ii() -> String {
    let caller = require_authenticated_caller();
    let sol_pk_str = ii_wallet_seed_for_principal(&caller).await;
    let subaccount = derive_subaccount(&sol_pk_str);
    let account = AccountIdentifier::new(&canister_id(), &subaccount);
    hex::encode(account.as_ref())
}

#[update]
async fn get_sol_balance_ii() -> u64 {
    let caller = require_authenticated_caller();
    let pubkey_str = bs58::encode(ii_sol_pubkey_for_principal(&caller).await).into_string();
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
    let caller = require_authenticated_caller();
    let sol_pk_str = ii_wallet_seed_for_principal(&caller).await;
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
    let caller = require_authenticated_caller();
    let sol_pk_str = ii_wallet_seed_for_principal(&caller).await;
    read_or_init_nonce(&sol_pk_str)
}

#[update]
async fn transfer_doge_ii(to: String, amount: u64) -> String {
    let caller = require_authenticated_caller();
    let sol_pk_str = ii_wallet_seed_for_principal(&caller).await;
    let current_nonce = read_or_init_nonce(&sol_pk_str);

    let material = match ii_doge_key_material(&caller).await {
        Ok(material) => material,
        Err(e) => return format!("DOGE wallet derivation failed: {e}"),
    };

    let txid = match doge_transfer_with_material(&material, &to, amount).await {
        Ok(txid) => txid,
        Err(e) => return format!("Send failed: {e}"),
    };

    NONCE_MAP.with(|map| {
        let mut map = map.borrow_mut();
        map.insert(sol_pk_str.clone(), current_nonce + 1);
    });

    format!("Transfer successful: DOGE txid {}", txid)
}

#[update]
async fn transfer_ii(to: String, amount: u64) -> String {
    let caller = require_authenticated_caller();
    let sol_pk_str = ii_wallet_seed_for_principal(&caller).await;
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
    let caller = require_authenticated_caller();
    let sol_pk_str = ii_wallet_seed_for_principal(&caller).await;
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

    let from_pk = ii_sol_pubkey_for_principal(&caller).await;
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
        derivation_path: vec![caller.as_slice().to_vec()],
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
async fn get_doge_deposit_address(sol_pubkey: String) -> String {
    doge_key_material_from_wallet(&sol_pubkey)
        .await
        .unwrap_or_else(|e| trap(&format!("Failed to derive DOGE address: {e}")))
        .address
}

#[update]
async fn get_doge_balance(sol_pubkey: String) -> u64 {
    let material = match doge_key_material_from_wallet(&sol_pubkey).await {
        Ok(material) => material,
        Err(e) => {
            ic_cdk::println!("get_doge_balance derive error: {}", e);
            return 0;
        }
    };

    match doge_get_balance_satoshis(&material.address).await {
        Ok(balance) => balance,
        Err(e) => {
            ic_cdk::println!("get_doge_balance error: {}", e);
            0
        }
    }
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
    if let Err(e) = require_phantom_signature(&sol_pubkey, message.as_bytes(), &signature) {
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
async fn transfer_doge(to: String, amount: u64, sol_pubkey: String, signature: Vec<u8>, nonce: u64) -> String {
    let current_nonce = get_nonce(sol_pubkey.clone());
    if nonce != current_nonce {
        return "Invalid nonce".to_string();
    }

    let message = format!("transfer_doge to {} amount {} nonce {}", to, amount, nonce);
    if let Err(e) = require_phantom_signature(&sol_pubkey, message.as_bytes(), &signature) {
        return e;
    }

    let material = match doge_key_material_from_wallet(&sol_pubkey).await {
        Ok(material) => material,
        Err(e) => return format!("DOGE wallet derivation failed: {e}"),
    };

    let txid = match doge_transfer_with_material(&material, &to, amount).await {
        Ok(txid) => txid,
        Err(e) => return format!("Send failed: {e}"),
    };

    NONCE_MAP.with(|map| {
        let mut map = map.borrow_mut();
        map.insert(sol_pubkey.clone(), current_nonce + 1);
    });

    format!("Transfer successful: DOGE txid {}", txid)
}

#[update]
async fn transfer_sol(to: String, amount: u64, sol_pubkey: String, signature: Vec<u8>, nonce: u64) -> String {
    let current_nonce = get_nonce(sol_pubkey.clone());
    if nonce != current_nonce {
        return "Invalid nonce".to_string();
    }

    let message = format!("transfer_sol to {} amount {} nonce {} service_fee {}", to, amount, nonce, SERVICE_FEE_SOL).into_bytes();
    if let Err(e) = require_phantom_signature(&sol_pubkey, &message, &signature) {
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
