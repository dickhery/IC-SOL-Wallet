// src/sol_icp_poc_frontend/assets/main.js
import { Actor, HttpAgent } from "@dfinity/agent";
import { AuthClient } from "@dfinity/auth-client";
import idlFactory from "./sol_icp_poc_backend.idl.js";

const host = process.env.DFX_NETWORK === "ic" ? "https://ic0.app" : "http://localhost:4943";
const canisterId = process.env.CANISTER_ID_SOL_ICP_POC_BACKEND;

let authClient = null;
let identity = null;
let agent = null;
let actor = null;

let authMode = null; // "ii" | "phantom"
let solPubkey = null;

// ---- UI helpers ----
function setVisible(id, visible) {
  document.getElementById(id).style.display = visible ? "block" : "none";
}
function uiSet(id, value) {
  document.getElementById(id).innerText = value;
}
function alertSet(cls, msg) {
  const el = document.getElementById("alerts");
  el.className = cls;
  el.textContent = msg || "";
}
const showOk = (m) => alertSet("ok", m);
const showWarn = (m) => alertSet("warn", m);
const showErr = (m) => alertSet("err", m);
const showMuted = (m) => alertSet("muted", m);

function normalizeAgentError(e) {
  const s = (e?.message || String(e || "")).trim();
  if (/Request timed out after/i.test(s)) {
    return "Request timed out. This may be due to network delays or consensus issues. The operation may still succeed—refresh balances in a few seconds.";
  }
  if (/processing/i.test(s) && /Request ID:/i.test(s)) {
    return "The network is processing the call. Refresh balances shortly.";
  }
  if (/inconsistent/i.test(s) || /consensus/i.test(s)) {
    return "Network variance prevented consensus. Retry the operation.";
  }
  if (/blockhash/i.test(s)) {
    return "Blockhash retrieval failed; network may be busy. Retry.";
  }
  return s;
}

function friendlyTry(fn, onErr) {
  return fn().catch((e) => {
    const msg = normalizeAgentError(e);
    if (onErr) onErr(msg, e);
    else showErr(msg);
    throw e; // keep behavior for upstream catch/finally
  });
}

// Add timeout wrapper for calls
async function withTimeout(promise, ms = 900000) {  // 15 minutes
  let timeoutId;
  const timeoutPromise = new Promise((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(`Timed out after ${ms} ms`)), ms);
  });
  return Promise.race([promise, timeoutPromise]).finally(() => clearTimeout(timeoutId));
}

async function makeAgentAndActor() {
  agent = new HttpAgent({ host, identity: identity ?? undefined });
  if (process.env.DFX_NETWORK !== "ic") {
    await agent.fetchRootKey();
  }
  actor = Actor.createActor(idlFactory, { agent, canisterId });
}

async function initAuthIfNeeded() {
  if (!authClient) authClient = await AuthClient.create();
}

// ---- Phantom provider ----
function getProvider() {
  if ("phantom" in window) {
    const provider = window.phantom?.solana;
    if (provider?.isPhantom) return provider;
  }
  window.open("https://phantom.app/", "_blank");
}
const provider = getProvider();

// ---- fees & constants ----
const serviceFeeICP = 0.0001;         // ICP fee for ICP transfers
const serviceFeeSolICP = 0.0002;      // ICP fee for SOL ops
const icpLedgerFee = 0.0001;          // ICP ledger fee to move service fee
const networkFeeICP = 0.0002;         // two ledger ops in ICP send flow
const solanaFeeApprox = 0.000005;     // SOL fee approx
const serviceFeeE8s = BigInt(Math.round(serviceFeeICP * 1e8));
const serviceFeeSolE8s = BigInt(Math.round(serviceFeeSolICP * 1e8));

// ---- Throttle state for refreshes ----
let lastSolRefreshMs = 0;
let solRefreshInFlight = false;
let lastIcpRefreshMs = 0;
let icpRefreshInFlight = false;
const COOLDOWN_MS = 10_000;

// Shared refresh that respects auth mode + cooldowns
async function refreshSolBalance(force = false) {
  const now = Date.now();
  if (!force && (now - lastSolRefreshMs) < COOLDOWN_MS) {
    const wait = Math.ceil((COOLDOWN_MS - (now - lastSolRefreshMs)) / 1000);
    showWarn(`Please wait ~${wait}s before refreshing SOL again.`);
    return;
  }
  if (solRefreshInFlight) {
    showMuted("Refreshing SOL…");
    return;
  }
  solRefreshInFlight = true;
  const button = document.getElementById("get_sol");
  if (button) { button.disabled = true; }

  try {
    let lam;
    if (authMode === "ii") {
      lam = await withTimeout(friendlyTry(() => actor.get_sol_balance_ii(), (m) => showWarn(m)));
    } else if (authMode === "phantom") {
      if (!solPubkey) return showWarn("Connect Phantom first");
      lam = await withTimeout(friendlyTry(() => actor.get_sol_balance(solPubkey), (m) => showWarn(m)));
    } else {
      showWarn("Pick an auth mode to refresh SOL.");
      return;
    }
    uiSet("sol_balance", `SOL Balance: ${(Number(lam)/1e9).toFixed(9)} SOL`);
    showMuted("SOL balance updated.");
    lastSolRefreshMs = Date.now();
  } catch (e) {
    if (String(e).includes("Timed out")) {
      showWarn("SOL refresh timed out. Network may be slow—try again in a minute.");
    } else {
      showErr(normalizeAgentError(e));
    }
  } finally {
    solRefreshInFlight = false;
    if (button) { button.disabled = false; }
  }
}

async function refreshIcpBalance(force = false) {
  const now = Date.now();
  if (!force && (now - lastIcpRefreshMs) < COOLDOWN_MS) {
    const wait = Math.ceil((COOLDOWN_MS - (now - lastIcpRefreshMs)) / 1000);
    showWarn(`Please wait ~${wait}s before refreshing ICP again.`);
    return;
  }
  if (icpRefreshInFlight) {
    showMuted("Refreshing ICP…");
    return;
  }
  icpRefreshInFlight = true;
  const button = document.getElementById("refresh_icp");
  if (button) { button.disabled = true; }

  try {
    let e8s;
    if (authMode === "ii") {
      e8s = await withTimeout(friendlyTry(() => actor.get_balance_ii(), (m) => showWarn(m)));
    } else if (authMode === "phantom") {
      if (!solPubkey) return showWarn("Connect Phantom first");
      e8s = await withTimeout(friendlyTry(() => actor.get_balance(solPubkey), (m) => showWarn(m)));
    } else {
      showWarn("Pick an auth mode to refresh ICP.");
      return;
    }
    uiSet("balance", `ICP Balance: ${(Number(e8s)/1e8).toFixed(8)} ICP`);
    showMuted("ICP balance updated.");
    lastIcpRefreshMs = Date.now();
  } catch (e) {
    if (String(e).includes("Timed out")) {
      showWarn("ICP refresh timed out. Network may be slow—try again in a minute.");
    } else {
      showErr(normalizeAgentError(e));
    }
  } finally {
    icpRefreshInFlight = false;
    if (button) { button.disabled = false; }
  }
}

async function refreshBothBalances(force = false) {
  await Promise.allSettled([
    refreshIcpBalance(force),
    refreshSolBalance(force),
  ]);
}

// Clear all dynamic text/inputs except latest-tx
function clearAllExceptTx() {
  ["deposit", "balance", "sol_deposit", "sol_balance", "pid", "pubkey", "ii_status", "status"].forEach(id => uiSet(id, ""));
  ["to", "amount", "to_sol", "amount_sol"].forEach(id => document.getElementById(id).value = "");
}

// ---- Auth mode switching ----
function enterIiUi() {
  setVisible("ii_block", true);
  setVisible("phantom_block", false);
  uiSet("mode_status", "Mode: Internet Identity");
  alertSet("", "");
}
function enterPhantomUi() {
  setVisible("phantom_block", true);
  setVisible("ii_block", false);
  uiSet("mode_status", "Mode: Phantom");
  alertSet("", "");
}

document.getElementById("mode_ii").onclick = async () => {
  clearAllExceptTx();
  if (authMode === "phantom") {
    try { await provider.disconnect(); } catch {}
    solPubkey = null;
  }
  authMode = "ii";
  await initAuthIfNeeded();
  await makeAgentAndActor();
  enterIiUi();
};

document.getElementById("mode_phantom").onclick = async () => {
  clearAllExceptTx();
  if (authMode === "ii") {
    if (!authClient) await initAuthIfNeeded();
    try { await authClient.logout(); } catch {}
    identity = null;
  }
  authMode = "phantom";
  await makeAgentAndActor();
  enterPhantomUi();
};

// ---- II login/logout ----
document.getElementById("ii_login").onclick = async () => {
  if (authMode !== "ii") return alert("Switch to Internet Identity mode first");
  await initAuthIfNeeded();
  const opts = { identityProvider: "https://identity.ic0.app" };
  authClient.login({
    ...opts,
    maxTimeToLive: BigInt(7) * BigInt(24*60*60*1_000_000_000),
    onSuccess: async () => {
      identity = authClient.getIdentity();
      await makeAgentAndActor();
      try {
        const prin = await actor.whoami();
        uiSet("ii_status", `Signed in as: ${prin}`);

        const dep = await friendlyTry(() => actor.get_deposit_address_ii(), (m) => showWarn(m));
        uiSet("deposit", `ICP Deposit to: ${dep}`);
        const solDep = await friendlyTry(() => actor.get_sol_deposit_address_ii(), (m) => showWarn(m));
        uiSet("sol_deposit", `SOL Deposit to: ${solDep}`);
        uiSet("pid", `ICP Principal: ${prin}`);

        await refreshBothBalances(true);
        showOk("Logged in with Internet Identity.");
      } catch (e) {
        uiSet("ii_status", `Signed in (fetch error).`);
        showWarn(normalizeAgentError(e));
      }
    },
    onError: (err) => {
      showErr(`II login failed: ${normalizeAgentError(err)}`);
    }
  });
};

document.getElementById("ii_logout").onclick = async () => {
  await initAuthIfNeeded();
  await authClient.logout();
  identity = null;
  await makeAgentAndActor();
  uiSet("ii_status", "Not signed in");
  uiSet("pid", "");
  showMuted("Logged out of Internet Identity.");
};

// ---- Phantom connect/logout ----
document.getElementById("connect").onclick = async () => {
  if (authMode !== "phantom") return alert("Switch to Phantom mode first");
  try {
    const resp = await provider.connect();
    solPubkey = resp.publicKey.toString();
    uiSet("pubkey", `Sol Pubkey: ${solPubkey} (Solana Mainnet)`);

    const deposit = await friendlyTry(() => actor.get_deposit_address(solPubkey), (m) => showWarn(m));
    uiSet("deposit", `ICP Deposit to: ${deposit} (Send ICP here)`);
    const solDeposit = await friendlyTry(() => actor.get_sol_deposit_address(solPubkey), (m) => showWarn(m));
    uiSet("sol_deposit", `SOL Deposit to: ${solDeposit} (Mainnet; send SOL here)`);

    await refreshBothBalances(true);
    showOk("Connected to Phantom.");
  } catch (err) {
    showErr(`Phantom connect error: ${normalizeAgentError(err)}`);
  }
};

document.getElementById("logout").onclick = async () => {
  try { await provider.disconnect(); } catch {}
  solPubkey = null;
  ["pubkey","pid","deposit","balance","sol_deposit","sol_balance"].forEach(id => uiSet(id, ""));
  showMuted("Disconnected Phantom. To prevent auto-reconnect, revoke in Phantom settings.");
};

// ---- Link wallet to II (optional) ----
document.getElementById("link_wallet").onclick = async () => {
  if (!solPubkey) return alert("Connect Phantom first");
  try {
    if (!authClient) await initAuthIfNeeded();
    if (!(await authClient.isAuthenticated())) return alert("Login with Internet Identity (II mode) to link");
    const prin = await actor.whoami();
    const message = `link ${prin}`;
    const encoded = new TextEncoder().encode(message);
    const signed = await provider.signMessage(encoded, "utf8");
    const signature = signed.signature;

    const res = await friendlyTry(() => actor.link_sol_pubkey(solPubkey, Array.from(signature)));
    showOk(res);
  } catch (err) {
    showErr(`Link error: ${normalizeAgentError(err)}`);
  }
};

// ---- ICP send ----
let sendingIcp = false;
document.getElementById("send").onclick = async () => {
  if (sendingIcp) return showWarn("ICP send already in progress.");
  sendingIcp = true;
  const button = document.getElementById("send");
  button.disabled = true; button.innerText = 'Processing... (may take 30s)';

  let initialNonce;
  try {
    const to = document.getElementById("to").value;
    const amountICP = document.getElementById("amount").value;
    if (isNaN(parseFloat(amountICP)) || parseFloat(amountICP) < 0) {
      throw new Error("Invalid amount");
    }
    const amount = BigInt(Math.round(parseFloat(amountICP) * 1e8));

    if (authMode === "ii") {
      initialNonce = await actor.get_nonce_ii();

      const totalICP = parseFloat(amountICP) + networkFeeICP + serviceFeeICP;
      const confirmMsg = `Confirm transaction (II mode):\nTo: ${to}\nAmount: ${amountICP} ICP\nNetwork fee: ${networkFeeICP} ICP\nService fee: ${serviceFeeICP} ICP\nTotal deduction: ${totalICP.toFixed(8)} ICP`;
      if (!window.confirm(confirmMsg)) throw new Error("Cancelled");

      const result = await withTimeout(friendlyTry(() => actor.transfer_ii(to, amount), (m) => showWarn(m)));
      displayResult(result);
      await refreshBothBalances(true);
      if (result.startsWith("Transfer successful")) {
        document.getElementById("to").value = '';
        document.getElementById("amount").value = '';
        showOk("ICP transfer complete. Balances updated.");
      } else {
        showWarn(result);
      }
      return;
    }

    if (authMode === "phantom") {
      if (!solPubkey) throw new Error("Connect Phantom first");

      initialNonce = await actor.get_nonce(solPubkey);

      const amountICPNum = parseFloat(amountICP);
      const totalICP = amountICPNum + networkFeeICP + serviceFeeICP;
      const confirmMsg = `Confirm transaction (Phantom mode):\nTo: ${to}\nAmount: ${amountICP} ICP\nNetwork fee: ${networkFeeICP} ICP\nService fee: ${serviceFeeICP} ICP\nTotal deduction: ${totalICP.toFixed(8)} ICP`;
      if (!window.confirm(confirmMsg)) throw new Error("Cancelled");

      const message = `transfer to ${to} amount ${amount} nonce ${initialNonce} service_fee ${serviceFeeE8s}`;
      const encodedMessage = new TextEncoder().encode(message);
      const signed = await provider.signMessage(encodedMessage, "utf8");
      const signature = signed.signature;

      const result = await withTimeout(friendlyTry(() => actor.transfer(to, amount, solPubkey, Array.from(signature), initialNonce), (m) => showWarn(m)));
      displayResult(result);
      await refreshBothBalances(true);

      if (result.startsWith("Transfer successful")) {
        document.getElementById("to").value = '';
        document.getElementById("amount").value = '';
        showOk("ICP transfer complete. Balances updated.");
      } else {
        showWarn(result);
      }
    }
  } catch (err) {
    const msg = (err?.message || String(err || "")).toLowerCase();
    if (msg.includes("timed out") || msg.includes("processing")) {
      await confirmAfterTimeout(initialNonce, "ICP");
    } else if (err.message === "Cancelled") {
      // user cancelled
    } else {
      showErr(`ICP send error: ${normalizeAgentError(err)}`);
    }
  } finally {
    sendingIcp = false;
    button.disabled = false; button.innerText = 'Send ICP';
  }
};

// ---- SOL read/send ----
document.getElementById("get_sol").onclick = async () => {
  await refreshSolBalance(false);
};

document.getElementById("refresh_icp").onclick = async () => {
  await refreshIcpBalance(false);
};

let sendingSol = false;
document.getElementById("send_sol").onclick = async () => {
  if (sendingSol) return showWarn("SOL send already in progress.");
  sendingSol = true;
  const button = document.getElementById("send_sol");
  button.disabled = true; button.innerText = 'Processing... (may take 30s)';

  let initialNonce;
  try {
    const to_sol = document.getElementById("to_sol").value;
    const amountSOL = document.getElementById("amount_sol").value;
    if (isNaN(parseFloat(amountSOL)) || parseFloat(amountSOL) < 0) {
      throw new Error("Invalid amount");
    }
    const amountLam = BigInt(Math.round(parseFloat(amountSOL) * 1e9));

    if (authMode === "ii") {
      initialNonce = await actor.get_nonce_ii();
      const totalSOL = parseFloat(amountSOL) + solanaFeeApprox;
      const totalIcpForSol = serviceFeeSolICP + icpLedgerFee;
      const confirmMsg = `Confirm SOL transaction (II mode):\nTo: ${to_sol}\nAmount: ${amountSOL} SOL\nSolana fee: ~${solanaFeeApprox} SOL\nICP ledger fee: ${icpLedgerFee} ICP\nService fee: ${serviceFeeSolICP} ICP\nTotal SOL deduction: ${totalSOL.toFixed(9)} SOL\nTotal ICP deduction: ${totalIcpForSol.toFixed(4)} ICP`;
      if (!window.confirm(confirmMsg)) throw new Error("Cancelled");

      const result = await withTimeout(friendlyTry(() => actor.transfer_sol_ii(to_sol, amountLam), (m) => showWarn(m)));
      displayResult(result);
      await refreshBothBalances(true);
      if (result.startsWith("Transfer successful")) {
        document.getElementById("to_sol").value = '';
        document.getElementById("amount_sol").value = '';
        showOk("SOL transfer complete. Balances updated.");
      } else {
        showWarn(result);
      }
      return;
    }

    if (authMode === "phantom") {
      if (!solPubkey) throw new Error("Connect first");
      initialNonce = await actor.get_nonce(solPubkey);

      const amountSOLNum = parseFloat(amountSOL);
      const totalSOL = amountSOLNum + solanaFeeApprox;
      const totalIcpForSol = serviceFeeSolICP + icpLedgerFee;
      const confirmMsg = `Confirm SOL transaction (Phantom mode):\nTo: ${to_sol}\nAmount: ${amountSOL} SOL\nSolana fee: ~${solanaFeeApprox} SOL\nICP ledger fee: ${icpLedgerFee} ICP\nService fee: ${serviceFeeSolICP} ICP\nTotal SOL deduction: ${totalSOL.toFixed(9)} SOL\nTotal ICP deduction: ${totalIcpForSol.toFixed(4)} ICP`;
      if (!window.confirm(confirmMsg)) throw new Error("Cancelled");

      const message = `transfer_sol to ${to_sol} amount ${amountLam} nonce ${initialNonce} service_fee ${serviceFeeSolE8s}`;
      const encodedMessage = new TextEncoder().encode(message);
      const signed = await provider.signMessage(encodedMessage, "utf8");
      const signature = signed.signature;

      const result = await withTimeout(friendlyTry(() => actor.transfer_sol(to_sol, amountLam, solPubkey, Array.from(signature), initialNonce), (m) => showWarn(m)));
      displayResult(result);
      await refreshBothBalances(true);

      if (result.startsWith("Transfer successful")) {
        document.getElementById("to_sol").value = '';
        document.getElementById("amount_sol").value = '';
        showOk("SOL transfer complete. Balances updated.");
      } else {
        showWarn(result);
      }
    }
  } catch (err) {
    const msg = (err?.message || String(err || "")).toLowerCase();
    if (msg.includes("timed out") || msg.includes("processing")) {
      await confirmAfterTimeout(initialNonce, "SOL");
    } else if (err.message === "Cancelled") {
      // noop
    } else {
      showErr(`SOL send error: ${normalizeAgentError(err)}`);
    }
  } finally {
    sendingSol = false;
    button.disabled = false; button.innerText = 'Send SOL';
  }
};

// Polling function for nonce change (to confirm TX success after timeout)
async function pollNonceForChange(initialNonce, maxAttempts = 10, intervalMs = 15000) {
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    await new Promise(resolve => setTimeout(resolve, intervalMs));
    try {
      const currentNonce = authMode === "ii" ? await actor.get_nonce_ii() : await actor.get_nonce(solPubkey);
      if (currentNonce > initialNonce) {
        return true; // success
      }
    } catch {}
    showMuted(`Polling for confirmation (${attempt}/${maxAttempts})...`);
  }
  return false;
}

// Helper for timeout/processing -> confirm via nonce
async function confirmAfterTimeout(initialNonce, assetType = "ICP") {
  showMuted(`${assetType} send submitted, waiting for network confirmation...`);
  const success = await pollNonceForChange(initialNonce, 12, 10000);
  if (success) {
    displayResult("Transfer successful (confirmed via nonce change)");
    await refreshBothBalances(true);
    showOk(`${assetType} transfer complete (delayed confirmation). Balances updated.`);
  } else {
    showWarn(`${assetType} send timed out and no confirmation detected. Check explorer or retry.`);
  }
}

// Safer result classification
function displayResult(res) {
  const txDiv = document.getElementById('latest-tx');
  let html = res;
  let cls = 'muted';

  const lower = res.toLowerCase();
  const isSuccess = res.startsWith('Transfer successful');
  const isHardFail = res.startsWith('Transfer failed') || res.startsWith('Send failed');
  const isErrorish = !isSuccess && /error/.test(lower);

  if (isSuccess) {
    cls = 'ok';
    const blockMatch = res.match(/block (\d+)/);
    if (blockMatch) {
      const block = blockMatch[1];
      const link = `https://dashboard.internetcomputer.org/`;
      html += ` <a href="${link}" target="_blank">View on ICP Dashboard</a>`;
    }
    const txidMatch = res.match(/txid (\S+)/);
    if (txidMatch) {
      const txid = txidMatch[1];
      const link = `https://explorer.solana.com/tx/${txid}`;
      html += ` <a href="${link}" target="_blank">View on Solana Explorer</a>`;
    }
  } else if (isHardFail) {
    cls = 'err';
  } else if (isErrorish) {
    cls = 'warn';
  }

  txDiv.className = cls;
  txDiv.innerHTML = html;
}

// ---- Boot ----
await makeAgentAndActor();
uiSet("mode_status", "Pick a mode: Internet Identity or Phantom");
showMuted("Ready.");
document.getElementById("latest-tx").innerHTML = "No transactions yet.";
