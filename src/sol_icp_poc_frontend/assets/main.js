import { Actor, HttpAgent } from "@dfinity/agent";
import { AuthClient } from "@dfinity/auth-client";
import idlFactory from "./sol_icp_poc_backend.idl.js";

const isMainnet = process.env.DFX_NETWORK === "ic";
const host = isMainnet ? "https://icp-api.io" : "http://127.0.0.1:4943";
const canisterId =
  process.env.CANISTER_ID_SOL_ICP_POC_BACKEND || "f4kcz-fqaaa-aaaap-an3hq-cai";

const serviceFeeICP = 0.0001;
const serviceFeeSolICP = 0.0002;
const icpLedgerFee = 0.0001;
const networkFeeICP = 0.0002;
const solanaFeeApprox = 0.000005;
const serviceFeeE8s = BigInt(Math.round(serviceFeeICP * 1e8));
const serviceFeeSolE8s = BigInt(Math.round(serviceFeeSolICP * 1e8));
const refreshCooldownMs = 10_000;
const textEncoder = new TextEncoder();

let authClient = null;
let identity = null;
let agent = null;
let actor = null;
let authMode = null;
let solPubkey = null;
let lastIcpRefreshMs = 0;
let lastSolRefreshMs = 0;
let icpRefreshInFlight = false;
let solRefreshInFlight = false;
let sendingIcp = false;
let sendingSol = false;

const $ = (id) => document.getElementById(id);

function setText(id, value) {
  const el = $(id);
  if (el) {
    el.textContent = value ?? "";
  }
}

function setBadge(id, label, tone) {
  const el = $(id);
  if (!el) return;
  el.textContent = label;
  el.className = `status-badge ${tone}`;
}

function syncAuthModeFromState() {
  authMode = identity ? "ii" : solPubkey ? "phantom" : null;
  setText(
    "mode_status",
    authMode === "ii"
      ? "Active wallet: Internet Identity"
      : authMode === "phantom"
        ? "Active wallet: Phantom"
        : "Active wallet: no session connected"
  );
}

function alertSet(tone, message) {
  const el = $("alerts");
  el.className = tone ? `notice ${tone}` : "notice";
  el.textContent = message || "";
}

function showOk(message) {
  alertSet("ok", message);
}

function showWarn(message) {
  alertSet("warn", message);
}

function showErr(message) {
  alertSet("err", message);
}

function showMuted(message) {
  alertSet("muted", message);
}

function resetWalletDisplay(reason, hint) {
  setText("pid", reason);
  setText("deposit", "ICP deposit address: waiting for authentication");
  setText("balance", "ICP Balance: --");
  setText("sol_deposit", "SOL deposit address: waiting for authentication");
  setText("sol_balance", "SOL Balance: --");
  setText("wallet_hint", hint);
}

function resetLatestTransaction() {
  const latest = $("latest-tx");
  latest.className = "transaction-card muted";
  latest.textContent = "No transactions yet.";
}

function renderLatestTransaction(message, tone = "muted", href = null, hrefLabel = null) {
  const latest = $("latest-tx");
  latest.className = `transaction-card ${tone}`;
  latest.textContent = "";

  const text = document.createElement("span");
  text.textContent = message;
  latest.append(text);

  if (href && hrefLabel) {
    latest.append(" ");
    const link = document.createElement("a");
    link.href = href;
    link.target = "_blank";
    link.rel = "noreferrer";
    link.textContent = hrefLabel;
    latest.append(link);
  }
}

function normalizeAgentError(error) {
  const text = (error?.message || String(error || "")).trim();
  if (/Authentication required/i.test(text)) {
    return "Sign in with Internet Identity before using the II-managed wallet.";
  }
  if (/Request timed out after/i.test(text)) {
    return "The request timed out. The network may still be processing it, so refresh again in a few seconds.";
  }
  if (/processing/i.test(text) && /Request ID:/i.test(text)) {
    return "The network is still processing the call. Refresh again shortly.";
  }
  if (/inconsistent/i.test(text) || /consensus/i.test(text)) {
    return "The network could not reach consensus for that response. Please retry.";
  }
  if (/blockhash/i.test(text)) {
    return "A fresh Solana blockhash was not available. Please retry.";
  }
  return text;
}

function friendlyTry(fn, onError) {
  return fn().catch((error) => {
    const message = normalizeAgentError(error);
    if (onError) {
      onError(message, error);
    } else {
      showErr(message);
    }
    throw error;
  });
}

async function withTimeout(promise, ms = 900000) {
  let timeoutId;
  const timeoutPromise = new Promise((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(`Timed out after ${ms} ms`)), ms);
  });
  return Promise.race([promise, timeoutPromise]).finally(() => clearTimeout(timeoutId));
}

function getIdentityProviderUrl() {
  return isMainnet
    ? "https://id.ai"
    : "http://rdmx6-jaaaa-aaaaa-aaadq-cai.localhost:4943";
}

async function initAuthIfNeeded() {
  if (!authClient) {
    authClient = await AuthClient.create();
  }
}

async function makeAgentAndActor() {
  agent = new HttpAgent({ host, identity: identity ?? undefined });
  if (!isMainnet) {
    await agent.fetchRootKey();
  }
  actor = Actor.createActor(idlFactory, { agent, canisterId });
}

async function logoutIiSession() {
  await initAuthIfNeeded();
  try {
    if (await authClient.isAuthenticated()) {
      await authClient.logout();
    }
  } catch {
    // Ignore logout failures and still clear local state.
  }
  identity = null;
}

async function disconnectPhantomSession() {
  const provider = getProvider();
  if (provider) {
    try {
      await provider.disconnect();
    } catch {
      // Ignore provider disconnect failures and still clear local state.
    }
  }
  solPubkey = null;
}

function getProvider(promptInstall = false) {
  const provider = window.phantom?.solana;
  if (provider?.isPhantom) {
    return provider;
  }
  if (promptInstall) {
    window.open("https://phantom.app/", "_blank", "noopener,noreferrer");
  }
  return null;
}

function bindProviderEvents() {
  const provider = getProvider();
  if (!provider || provider.__walletUiBound) {
    return;
  }

  provider.__walletUiBound = true;
  provider.on("disconnect", () => {
    solPubkey = null;
    renderConnectionState();
    if (!authMode) {
      resetWalletDisplay(
        "Phantom public key: connect your wallet to load the Phantom-managed account.",
        "Only one authentication method can be active at a time. Connect Phantom or sign in with Internet Identity."
      );
    }
  });
}

function renderConnectionState() {
  syncAuthModeFromState();
  setBadge("ii_badge", identity ? "Connected" : "Offline", identity ? "ok" : "muted");
  setBadge(
    "phantom_badge",
    solPubkey ? "Connected" : "Offline",
    solPubkey ? "ok" : "muted"
  );

  setText(
    "ii_status",
    identity
      ? "Internet Identity session ready."
      : "Sign in with Internet Identity to unlock the II-managed wallet."
  );
  setText(
    "status",
    solPubkey
      ? "Phantom connected on Solana mainnet."
      : "Connect Phantom to unlock the Phantom-managed wallet."
  );
  setText("pubkey", solPubkey ? `Phantom public key: ${solPubkey}` : "");
}

async function restoreIiSession() {
  await initAuthIfNeeded();
  if (await authClient.isAuthenticated()) {
    identity = authClient.getIdentity();
  }
}

async function restoreTrustedPhantomConnection() {
  if (identity) return;
  const provider = getProvider();
  if (!provider) return;

  bindProviderEvents();
  try {
    const response = await provider.connect({ onlyIfTrusted: true });
    solPubkey = response?.publicKey?.toString?.() || null;
  } catch {
    solPubkey = null;
  }
}

async function ensureActor() {
  if (!actor) {
    await makeAgentAndActor();
  }
}

async function hydrateIiWallet(forceBalances = true) {
  if (!identity) {
    resetWalletDisplay(
      "Internet Identity principal: sign in to load your wallet.",
      "Only one authentication method can be active at a time. Sign in with Internet Identity to use the II wallet."
    );
    return;
  }

  await makeAgentAndActor();

  try {
    const principal = await actor.whoami();
    const [deposit, solDeposit] = await Promise.all([
      friendlyTry(() => actor.get_deposit_address_ii()),
      friendlyTry(() => actor.get_sol_deposit_address_ii()),
    ]);

    setText("pid", `Internet Identity principal: ${principal}`);
    setText("deposit", `ICP deposit address: ${deposit}`);
    setText("sol_deposit", `SOL deposit address: ${solDeposit}`);
    setText(
      "wallet_hint",
      "This wallet is derived from your Internet Identity session."
    );

    if (forceBalances) {
      await refreshBothBalances(true, true);
    }
  } catch (error) {
    showWarn(normalizeAgentError(error));
  }
}

async function hydratePhantomWallet(forceBalances = true) {
  if (!solPubkey) {
    resetWalletDisplay(
      "Phantom public key: connect your wallet to load the Phantom-managed account.",
      "Only one authentication method can be active at a time. Connect Phantom to use the Phantom wallet."
    );
    return;
  }

  await ensureActor();

  try {
    const [deposit, solDeposit] = await Promise.all([
      friendlyTry(() => actor.get_deposit_address(solPubkey)),
      friendlyTry(() => actor.get_sol_deposit_address(solPubkey)),
    ]);

    setText("pid", `Phantom public key: ${solPubkey}`);
    setText("deposit", `ICP deposit address: ${deposit}`);
    setText("sol_deposit", `SOL deposit address: ${solDeposit}`);
    setText(
      "wallet_hint",
      "This wallet is derived from your Phantom public key and requires Phantom signatures for transfers."
    );

    if (forceBalances) {
      await refreshBothBalances(true, true);
    }
  } catch (error) {
    showWarn(normalizeAgentError(error));
  }
}

async function hydrateActiveWallet(forceBalances = true) {
  renderConnectionState();
  if (authMode === "ii") {
    await hydrateIiWallet(forceBalances);
  } else if (authMode === "phantom") {
    await hydratePhantomWallet(forceBalances);
  } else {
    resetWalletDisplay(
      "No active wallet session. Sign in with Internet Identity or connect Phantom to continue.",
      "Only one authentication method can be active at a time."
    );
  }
}

async function refreshIcpBalance(force = false, quiet = false) {
  const now = Date.now();
  if (!force && now - lastIcpRefreshMs < refreshCooldownMs) {
    const waitSeconds = Math.ceil((refreshCooldownMs - (now - lastIcpRefreshMs)) / 1000);
    if (!quiet) {
      showWarn(`Please wait about ${waitSeconds}s before refreshing ICP again.`);
    }
    return null;
  }
  if (icpRefreshInFlight) {
    return null;
  }

  if (!authMode) {
    setText("balance", "ICP Balance: --");
    if (!quiet) {
      showWarn("Sign in with Internet Identity or connect Phantom before refreshing ICP.");
    }
    return null;
  }
  if (authMode === "ii" && !identity) {
    setText("balance", "ICP Balance: --");
    if (!quiet) {
      showWarn("Sign in with Internet Identity before refreshing the II-managed ICP balance.");
    }
    return null;
  }
  if (authMode === "phantom" && !solPubkey) {
    setText("balance", "ICP Balance: --");
    if (!quiet) {
      showWarn("Connect Phantom before refreshing the Phantom-managed ICP balance.");
    }
    return null;
  }

  icpRefreshInFlight = true;
  $("refresh_icp").disabled = true;

  try {
    await ensureActor();
    const e8s =
      authMode === "ii"
        ? await withTimeout(
            friendlyTry(() => actor.get_balance_ii(), (message) => !quiet && showWarn(message))
          )
        : await withTimeout(
            friendlyTry(() => actor.get_balance(solPubkey), (message) => !quiet && showWarn(message))
          );
    setText("balance", `ICP Balance: ${(Number(e8s) / 1e8).toFixed(8)} ICP`);
    lastIcpRefreshMs = Date.now();
    if (!quiet) {
      showMuted("ICP balance updated.");
    }
    return e8s;
  } catch (error) {
    if (!quiet) {
      showErr(normalizeAgentError(error));
    }
    return null;
  } finally {
    icpRefreshInFlight = false;
    $("refresh_icp").disabled = false;
  }
}

async function refreshSolBalance(force = false, quiet = false) {
  const now = Date.now();
  if (!force && now - lastSolRefreshMs < refreshCooldownMs) {
    const waitSeconds = Math.ceil((refreshCooldownMs - (now - lastSolRefreshMs)) / 1000);
    if (!quiet) {
      showWarn(`Please wait about ${waitSeconds}s before refreshing SOL again.`);
    }
    return null;
  }
  if (solRefreshInFlight) {
    return null;
  }

  if (!authMode) {
    setText("sol_balance", "SOL Balance: --");
    if (!quiet) {
      showWarn("Sign in with Internet Identity or connect Phantom before refreshing SOL.");
    }
    return null;
  }
  if (authMode === "ii" && !identity) {
    setText("sol_balance", "SOL Balance: --");
    if (!quiet) {
      showWarn("Sign in with Internet Identity before refreshing the II-managed SOL balance.");
    }
    return null;
  }
  if (authMode === "phantom" && !solPubkey) {
    setText("sol_balance", "SOL Balance: --");
    if (!quiet) {
      showWarn("Connect Phantom before refreshing the Phantom-managed SOL balance.");
    }
    return null;
  }

  solRefreshInFlight = true;
  $("get_sol").disabled = true;

  try {
    await ensureActor();
    const lamports =
      authMode === "ii"
        ? await withTimeout(
            friendlyTry(() => actor.get_sol_balance_ii(), (message) => !quiet && showWarn(message))
          )
        : await withTimeout(
            friendlyTry(
              () => actor.get_sol_balance(solPubkey),
              (message) => !quiet && showWarn(message)
            )
          );
    setText("sol_balance", `SOL Balance: ${(Number(lamports) / 1e9).toFixed(9)} SOL`);
    lastSolRefreshMs = Date.now();
    if (!quiet) {
      showMuted("SOL balance updated.");
    }
    return lamports;
  } catch (error) {
    if (!quiet) {
      showErr(normalizeAgentError(error));
    }
    return null;
  } finally {
    solRefreshInFlight = false;
    $("get_sol").disabled = false;
  }
}

async function refreshBothBalances(force = false, quiet = false) {
  await Promise.allSettled([
    refreshIcpBalance(force, quiet),
    refreshSolBalance(force, quiet),
  ]);
}

async function pollNonceForChange(initialNonce, maxAttempts = 12, intervalMs = 10000) {
  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, intervalMs));

    try {
      await ensureActor();
      const currentNonce =
        authMode === "ii"
          ? await actor.get_nonce_ii()
          : await actor.get_nonce(solPubkey);

      if (Number(currentNonce) > Number(initialNonce)) {
        return true;
      }
    } catch {
      // Ignore intermittent polling failures and continue.
    }

    showMuted(`Waiting for confirmation (${attempt}/${maxAttempts})...`);
  }

  return false;
}

async function confirmAfterTimeout(initialNonce, assetType) {
  showMuted(`${assetType} transfer submitted. Waiting for network confirmation...`);
  const success = await pollNonceForChange(initialNonce);

  if (success) {
    renderLatestTransaction(
      "Transfer successful (confirmed after delayed network response).",
      "ok"
    );
    await refreshBothBalances(true, true);
    showOk(`${assetType} transfer completed after a delayed confirmation.`);
  } else {
    showWarn(
      `${assetType} transfer timed out and no nonce change was detected. Check the explorer or retry after a short wait.`
    );
  }
}

function displayResult(result) {
  if (result.startsWith("Transfer successful")) {
    const txidMatch = result.match(/txid (\S+)/);
    if (txidMatch) {
      renderLatestTransaction(
        result,
        "ok",
        `https://explorer.solana.com/tx/${txidMatch[1]}?cluster=mainnet-beta`,
        "View on Solana Explorer"
      );
    } else {
      renderLatestTransaction(result, "ok");
    }
    return;
  }

  if (result.startsWith("Transfer failed") || result.startsWith("Send failed")) {
    renderLatestTransaction(result, "err");
    return;
  }

  if (/error/i.test(result)) {
    renderLatestTransaction(result, "warn");
    return;
  }

  renderLatestTransaction(result, "muted");
}

function validatePositiveAmount(rawValue, label) {
  const parsed = Number.parseFloat(rawValue);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`Invalid ${label} amount`);
  }
  return parsed;
}

$("ii_login").onclick = async () => {
  await initAuthIfNeeded();
  authClient.login({
    identityProvider: getIdentityProviderUrl(),
    maxTimeToLive: BigInt(8) * BigInt(3_600_000_000_000),
    onSuccess: async () => {
      const hadPhantomSession = Boolean(solPubkey);
      if (hadPhantomSession) {
        await disconnectPhantomSession();
      }
      identity = authClient.getIdentity();
      await makeAgentAndActor();
      await hydrateActiveWallet(true);
      showOk(
        hadPhantomSession
          ? "Internet Identity connected. Phantom was disconnected to keep a single active session."
          : "Internet Identity connected."
      );
    },
    onError: (error) => {
      showErr(`Internet Identity login failed: ${normalizeAgentError(error)}`);
    },
  });
};

$("ii_logout").onclick = async () => {
  await logoutIiSession();
  await makeAgentAndActor();
  await hydrateActiveWallet(false);
  showMuted("Internet Identity disconnected.");
};

$("connect").onclick = async () => {
  const provider = getProvider(true);
  if (!provider) return;

  bindProviderEvents();

  try {
    const response = await provider.connect();
    const hadIiSession = Boolean(identity);
    if (hadIiSession) {
      await logoutIiSession();
    }
    solPubkey = response.publicKey.toString();
    await makeAgentAndActor();
    await hydrateActiveWallet(true);
    showOk(
      hadIiSession
        ? "Phantom connected. Internet Identity was logged out to keep a single active session."
        : "Phantom connected."
    );
  } catch (error) {
    showErr(`Phantom connection failed: ${normalizeAgentError(error)}`);
  }
};

$("logout").onclick = async () => {
  await disconnectPhantomSession();
  await makeAgentAndActor();
  await hydrateActiveWallet(false);
  showMuted("Phantom disconnected.");
};

$("refresh_icp").onclick = async () => {
  await refreshIcpBalance(false, false);
};

$("get_sol").onclick = async () => {
  await refreshSolBalance(false, false);
};

$("send").onclick = async () => {
  if (sendingIcp) {
    showWarn("An ICP transfer is already in progress.");
    return;
  }

  sendingIcp = true;
  $("send").disabled = true;
  $("send").textContent = "Sending ICP...";

  let initialNonce = null;

  try {
    const to = $("to").value.trim();
    const amountInput = $("amount").value.trim();
    const amountICP = validatePositiveAmount(amountInput, "ICP");
    const amount = BigInt(Math.round(amountICP * 1e8));

    if (!to) {
      throw new Error("Enter a destination account identifier for ICP.");
    }

    await ensureActor();

    if (authMode === "ii") {
      if (!identity) {
        throw new Error("Sign in with Internet Identity first.");
      }

      initialNonce = await actor.get_nonce_ii();
      const total = amountICP + networkFeeICP + serviceFeeICP;
      const confirmed = window.confirm(
        `Send ${amountICP.toFixed(8)} ICP?\n\nDestination: ${to}\nNetwork fee: ${networkFeeICP.toFixed(
          4
        )} ICP\nService fee: ${serviceFeeICP.toFixed(4)} ICP\nEstimated total deduction: ${total.toFixed(
          8
        )} ICP`
      );
      if (!confirmed) {
        throw new Error("Cancelled");
      }

      const result = await withTimeout(
        friendlyTry(() => actor.transfer_ii(to, amount), (message) => showWarn(message))
      );
      displayResult(result);
      await refreshBothBalances(true, true);
      showOk("ICP transfer submitted through Internet Identity mode.");
    } else if (authMode === "phantom") {
      if (!solPubkey) {
        throw new Error("Connect Phantom first.");
      }

      const provider = getProvider(true);
      if (!provider) return;

      initialNonce = await actor.get_nonce(solPubkey);
      const total = amountICP + networkFeeICP + serviceFeeICP;
      const confirmed = window.confirm(
        `Send ${amountICP.toFixed(8)} ICP?\n\nDestination: ${to}\nNetwork fee: ${networkFeeICP.toFixed(
          4
        )} ICP\nService fee: ${serviceFeeICP.toFixed(4)} ICP\nEstimated total deduction: ${total.toFixed(
          8
        )} ICP`
      );
      if (!confirmed) {
        throw new Error("Cancelled");
      }

      const message = `transfer to ${to} amount ${amount} nonce ${initialNonce} service_fee ${serviceFeeE8s}`;
      const signed = await provider.signMessage(textEncoder.encode(message), "utf8");
      const result = await withTimeout(
        friendlyTry(
          () => actor.transfer(to, amount, solPubkey, Array.from(signed.signature), initialNonce),
          (errorMessage) => showWarn(errorMessage)
        )
      );
      displayResult(result);
      await refreshBothBalances(true, true);
      showOk("ICP transfer submitted through Phantom mode.");
    } else {
      throw new Error("Sign in with Internet Identity or connect Phantom first.");
    }

    $("to").value = "";
    $("amount").value = "";
  } catch (error) {
    const message = (error?.message || String(error || "")).toLowerCase();
    if (message.includes("timed out") || message.includes("processing")) {
      await confirmAfterTimeout(initialNonce, "ICP");
    } else if (error.message !== "Cancelled") {
      showErr(`ICP transfer failed: ${normalizeAgentError(error)}`);
    }
  } finally {
    sendingIcp = false;
    $("send").disabled = false;
    $("send").textContent = "Send ICP";
  }
};

$("send_sol").onclick = async () => {
  if (sendingSol) {
    showWarn("A SOL transfer is already in progress.");
    return;
  }

  sendingSol = true;
  $("send_sol").disabled = true;
  $("send_sol").textContent = "Sending SOL...";

  let initialNonce = null;

  try {
    const to = $("to_sol").value.trim();
    const amountInput = $("amount_sol").value.trim();
    const amountSol = validatePositiveAmount(amountInput, "SOL");
    const amountLamports = BigInt(Math.round(amountSol * 1e9));

    if (!to) {
      throw new Error("Enter a destination Solana address.");
    }

    await ensureActor();

    if (authMode === "ii") {
      if (!identity) {
        throw new Error("Sign in with Internet Identity first.");
      }

      initialNonce = await actor.get_nonce_ii();
      const totalSol = amountSol + solanaFeeApprox;
      const totalIcp = serviceFeeSolICP + icpLedgerFee;
      const confirmed = window.confirm(
        `Send ${amountSol.toFixed(9)} SOL?\n\nDestination: ${to}\nEstimated Solana fee: ${solanaFeeApprox.toFixed(
          6
        )} SOL\nICP ledger fee: ${icpLedgerFee.toFixed(4)} ICP\nService fee: ${serviceFeeSolICP.toFixed(
          4
        )} ICP\nEstimated SOL deduction: ${totalSol.toFixed(
          9
        )} SOL\nEstimated ICP deduction: ${totalIcp.toFixed(4)} ICP`
      );
      if (!confirmed) {
        throw new Error("Cancelled");
      }

      const result = await withTimeout(
        friendlyTry(() => actor.transfer_sol_ii(to, amountLamports), (message) => showWarn(message))
      );
      displayResult(result);
      await refreshBothBalances(true, true);
      showOk("SOL transfer submitted through Internet Identity mode.");
    } else if (authMode === "phantom") {
      if (!solPubkey) {
        throw new Error("Connect Phantom first.");
      }

      const provider = getProvider(true);
      if (!provider) return;

      initialNonce = await actor.get_nonce(solPubkey);
      const totalSol = amountSol + solanaFeeApprox;
      const totalIcp = serviceFeeSolICP + icpLedgerFee;
      const confirmed = window.confirm(
        `Send ${amountSol.toFixed(9)} SOL?\n\nDestination: ${to}\nEstimated Solana fee: ${solanaFeeApprox.toFixed(
          6
        )} SOL\nICP ledger fee: ${icpLedgerFee.toFixed(4)} ICP\nService fee: ${serviceFeeSolICP.toFixed(
          4
        )} ICP\nEstimated SOL deduction: ${totalSol.toFixed(
          9
        )} SOL\nEstimated ICP deduction: ${totalIcp.toFixed(4)} ICP`
      );
      if (!confirmed) {
        throw new Error("Cancelled");
      }

      const message = `transfer_sol to ${to} amount ${amountLamports} nonce ${initialNonce} service_fee ${serviceFeeSolE8s}`;
      const signed = await provider.signMessage(textEncoder.encode(message), "utf8");
      const result = await withTimeout(
        friendlyTry(
          () =>
            actor.transfer_sol(
              to,
              amountLamports,
              solPubkey,
              Array.from(signed.signature),
              initialNonce
            ),
          (errorMessage) => showWarn(errorMessage)
        )
      );
      displayResult(result);
      await refreshBothBalances(true, true);
      showOk("SOL transfer submitted through Phantom mode.");
    } else {
      throw new Error("Sign in with Internet Identity or connect Phantom first.");
    }

    $("to_sol").value = "";
    $("amount_sol").value = "";
  } catch (error) {
    const message = (error?.message || String(error || "")).toLowerCase();
    if (message.includes("timed out") || message.includes("processing")) {
      await confirmAfterTimeout(initialNonce, "SOL");
    } else if (error.message !== "Cancelled") {
      showErr(`SOL transfer failed: ${normalizeAgentError(error)}`);
    }
  } finally {
    sendingSol = false;
    $("send_sol").disabled = false;
    $("send_sol").textContent = "Send SOL";
  }
};

await restoreIiSession();
await restoreTrustedPhantomConnection();
await makeAgentAndActor();
bindProviderEvents();
renderConnectionState();
resetLatestTransaction();
await hydrateActiveWallet(Boolean(identity || solPubkey));
showMuted("Ready.");
