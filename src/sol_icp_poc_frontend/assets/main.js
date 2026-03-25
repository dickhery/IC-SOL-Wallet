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
const dogeSatoshisPerDoge = 1e8;
const serviceFeeE8s = BigInt(Math.round(serviceFeeICP * 1e8));
const serviceFeeSolE8s = BigInt(Math.round(serviceFeeSolICP * 1e8));
const refreshCooldownMs = 10_000;
const textEncoder = new TextEncoder();
const phantomMobileConnectParam = "phantom_mobile_connect";
const phantomBrowseBaseUrl = "https://phantom.app/ul/browse";
const mobileUserAgentPattern = /android|iphone|ipad|ipod|iemobile|opera mini|mobile|blackberry/i;

let authClient = null;
let identity = null;
let agent = null;
let actor = null;
let authMode = null;
let solPubkey = null;
let icpDepositAddress = null;
let solDepositAddress = null;
let dogeDepositAddress = null;
let lastIcpRefreshMs = 0;
let lastSolRefreshMs = 0;
let lastDogeRefreshMs = 0;
let icpRefreshInFlight = false;
let solRefreshInFlight = false;
let dogeRefreshInFlight = false;
let sendingIcp = false;
let sendingSol = false;
let sendingDoge = false;
let connectingPhantom = false;

const $ = (id) => document.getElementById(id);
const compactValueMediaQuery = window.matchMedia("(max-width: 720px)");
const responsiveValueIds = ["pubkey", "pid", "deposit", "sol_deposit", "doge_deposit"];
const copyButtonConfigs = {
  copy_icp: {
    idleLabel: "Copy ICP Address",
    hasValue: () => Boolean(icpDepositAddress),
  },
  copy_sol: {
    idleLabel: "Copy SOL Address",
    hasValue: () => Boolean(solDepositAddress),
  },
  copy_doge: {
    idleLabel: "Copy DOGE Address",
    hasValue: () => Boolean(dogeDepositAddress),
  },
};
const copyButtonFeedbackTimers = new Map();
const copyButtonBusyIds = new Set();

function setText(id, value) {
  const el = $(id);
  if (el) {
    el.textContent = value ?? "";
  }
}

function compactMiddle(value, start = 10, end = 8) {
  if (!value || value.length <= start + end + 3) {
    return value || "";
  }
  return `${value.slice(0, start)}...${value.slice(-end)}`;
}

function setResponsiveValue(id, label, value, fallback = "") {
  const el = $(id);
  if (!el) return;
  el.dataset.label = label || "";
  el.dataset.fullValue = value || "";
  el.dataset.fallback = fallback || "";
  renderResponsiveValue(el);
}

function renderResponsiveValue(target) {
  const el = typeof target === "string" ? $(target) : target;
  if (!el) return;

  const label = el.dataset.label || "";
  const fullValue = el.dataset.fullValue || "";
  const fallback = el.dataset.fallback || "";

  if (!fullValue) {
    el.textContent = fallback;
    el.removeAttribute("title");
    return;
  }

  const visibleValue = compactValueMediaQuery.matches ? compactMiddle(fullValue) : fullValue;
  el.textContent = label ? `${label}: ${visibleValue}` : visibleValue;

  if (visibleValue !== fullValue) {
    el.title = fullValue;
  } else {
    el.removeAttribute("title");
  }
}

function renderResponsiveValues() {
  responsiveValueIds.forEach(renderResponsiveValue);
}

function isMobileDevice() {
  const userAgent = navigator.userAgent || navigator.vendor || "";
  if (mobileUserAgentPattern.test(userAgent)) {
    return true;
  }
  return navigator.maxTouchPoints > 1 && /Mac/i.test(navigator.platform || "");
}

function isPhantomReadyUrl() {
  return (
    window.location.protocol === "https:" ||
    window.location.hostname === "localhost" ||
    window.location.hostname === "127.0.0.1"
  );
}

function buildPhantomBrowseUrl() {
  const targetUrl = new URL(window.location.href);
  targetUrl.searchParams.set(phantomMobileConnectParam, "1");
  return `${phantomBrowseBaseUrl}/${encodeURIComponent(targetUrl.toString())}?ref=${encodeURIComponent(
    window.location.origin
  )}`;
}

function clearPhantomMobileConnectIntent() {
  const currentUrl = new URL(window.location.href);
  if (!currentUrl.searchParams.has(phantomMobileConnectParam)) {
    return;
  }

  currentUrl.searchParams.delete(phantomMobileConnectParam);
  const nextUrl = `${currentUrl.pathname}${currentUrl.search}${currentUrl.hash}`;
  window.history.replaceState({}, document.title, nextUrl || "/");
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

function getDefaultButtonLabel(id) {
  const button = $(id);
  if (!button) {
    return "";
  }

  if (!button.dataset.defaultLabel) {
    button.dataset.defaultLabel = button.textContent.trim();
  }

  return button.dataset.defaultLabel;
}

function setButtonState(id, label, { disabled, busy = false } = {}) {
  const button = $(id);
  if (!button) {
    return;
  }

  getDefaultButtonLabel(id);

  if (label != null) {
    button.textContent = label;
  }

  if (typeof disabled === "boolean") {
    button.disabled = disabled;
  }

  if (busy) {
    button.setAttribute("aria-busy", "true");
  } else {
    button.removeAttribute("aria-busy");
  }
}

function clearTransferFields(...fieldIds) {
  fieldIds.forEach((fieldId) => {
    const field = $(fieldId);
    if (field) {
      field.value = "";
    }
  });
}

function updateCopyButtons() {
  Object.entries(copyButtonConfigs).forEach(([id, config]) => {
    const button = $(id);
    if (!button) {
      return;
    }

    button.dataset.defaultLabel = config.idleLabel;

    if (copyButtonBusyIds.has(id) || copyButtonFeedbackTimers.has(id)) {
      button.disabled = true;
      return;
    }

    button.textContent = config.idleLabel;
    button.disabled = !config.hasValue();
    button.removeAttribute("aria-busy");
  });
}

function setWalletAddresses({ icp = null, sol = null, doge = null } = {}) {
  icpDepositAddress = icp;
  solDepositAddress = sol;
  dogeDepositAddress = doge;
  updateCopyButtons();
}

function resetWalletDisplay(reason, hint) {
  setWalletAddresses();
  setResponsiveValue("pid", "", "", reason);
  setResponsiveValue("deposit", "", "", "ICP deposit address: waiting for authentication");
  setText("balance", "ICP Balance: --");
  setResponsiveValue("sol_deposit", "", "", "SOL deposit address: waiting for authentication");
  setText("sol_balance", "SOL Balance: --");
  setResponsiveValue("doge_deposit", "", "", "DOGE deposit address: waiting for authentication");
  setText("doge_balance", "DOGE Balance: --");
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
  if (/sol https outcall failed/i.test(text) || /solana rpc/i.test(text)) {
    return "The backend could not fetch the SOL balance from Solana right now. Please retry in a few seconds.";
  }
  if (/doge/i.test(text) && /address/i.test(text)) {
    return "Enter a valid Dogecoin address.";
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

function getProvider() {
  const provider = window.phantom?.solana;
  if (provider?.isPhantom) {
    return provider;
  }
  return null;
}

function openPhantomDownloadPage() {
  window.open("https://phantom.app/download", "_blank", "noopener,noreferrer");
}

function openPhantomInAppBrowser() {
  if (!isPhantomReadyUrl()) {
    showWarn(
      "Phantom mobile needs this app on an HTTPS URL before it can inject the wallet. Deploy or tunnel the frontend, then try again."
    );
    return false;
  }

  showMuted(
    "Opening this page inside Phantom. If the connect sheet does not appear automatically, tap Connect Phantom after Phantom finishes loading."
  );
  window.location.assign(buildPhantomBrowseUrl());
  return true;
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

function updatePhantomConnectUi() {
  const connectButton = $("connect");
  const disconnectButton = $("logout");
  if (!connectButton || !disconnectButton) {
    return;
  }

  if (connectingPhantom) {
    connectButton.textContent = "Connecting Phantom...";
    connectButton.disabled = true;
    disconnectButton.disabled = true;
    setText("phantom_mobile_hint", "");
    return;
  }

  connectButton.disabled = false;
  disconnectButton.disabled = !solPubkey;

  const provider = getProvider();
  const mobile = isMobileDevice();

  if (solPubkey) {
    connectButton.textContent = "Reconnect Phantom";
    setText("phantom_mobile_hint", "");
    return;
  }

  if (provider) {
    connectButton.textContent = "Connect Phantom";
    setText(
      "phantom_mobile_hint",
      mobile
        ? "You are inside Phantom's in-app browser. Connecting here enables Phantom signatures for ICP, SOL, and DOGE transfers."
        : ""
    );
    return;
  }

  if (mobile) {
    connectButton.textContent = "Open in Phantom";
    setText(
      "phantom_mobile_hint",
      isPhantomReadyUrl()
        ? "Phantom only injects wallet access inside its in-app browser on mobile. Tapping the button will reopen this page there."
        : "Phantom mobile requires this app to be served from HTTPS before wallet injection is available."
    );
    return;
  }

  connectButton.textContent = "Get Phantom";
  setText("phantom_mobile_hint", "Desktop Phantom connections use the browser extension.");
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
      : connectingPhantom
        ? "Connecting to Phantom..."
        : getProvider()
          ? "Connect Phantom to unlock the Phantom-managed wallet."
          : isMobileDevice()
            ? isPhantomReadyUrl()
              ? "Mobile browser detected. Open this app in Phantom to connect your wallet."
              : "Mobile browser detected. Phantom needs this app on HTTPS before it can connect."
            : "Install Phantom to unlock the Phantom-managed wallet."
  );
  setResponsiveValue("pubkey", "Phantom public key", solPubkey, "");
  updatePhantomConnectUi();
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

async function connectPhantomWallet() {
  const provider = getProvider();
  if (!provider) {
    if (isMobileDevice()) {
      openPhantomInAppBrowser();
    } else {
      openPhantomDownloadPage();
    }
    return false;
  }

  if (connectingPhantom) {
    return false;
  }

  connectingPhantom = true;
  bindProviderEvents();
  renderConnectionState();

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
    return true;
  } catch (error) {
    showErr(`Phantom connection failed: ${normalizeAgentError(error)}`);
    return false;
  } finally {
    connectingPhantom = false;
    renderConnectionState();
  }
}

async function maybeContinuePhantomMobileConnect() {
  const currentUrl = new URL(window.location.href);
  if (currentUrl.searchParams.get(phantomMobileConnectParam) !== "1") {
    return { attempted: false, connected: false };
  }

  clearPhantomMobileConnectIntent();

  if (solPubkey) {
    return { attempted: true, connected: false };
  }

  if (!getProvider()) {
    showWarn(
      "This page opened without Phantom injection. Open the app from Phantom's in-app browser on an HTTPS URL and try again."
    );
    return { attempted: true, connected: false };
  }

  showMuted("Phantom is ready. Finishing wallet connection...");
  return { attempted: true, connected: await connectPhantomWallet() };
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
    const [deposit, solDeposit, dogeDeposit] = await Promise.all([
      friendlyTry(() => actor.get_deposit_address_ii()),
      friendlyTry(() => actor.get_sol_deposit_address_ii()),
      friendlyTry(() => actor.get_doge_deposit_address_ii()),
    ]);

    setWalletAddresses({ icp: deposit, sol: solDeposit, doge: dogeDeposit });
    setResponsiveValue("pid", "Internet Identity principal", String(principal));
    setResponsiveValue("deposit", "ICP deposit address", deposit);
    setResponsiveValue("sol_deposit", "SOL deposit address", solDeposit);
    setResponsiveValue("doge_deposit", "DOGE deposit address", dogeDeposit);
    setText(
      "wallet_hint",
      "This wallet is derived from your Internet Identity session across ICP, SOL, and DOGE."
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
    const [deposit, solDeposit, dogeDeposit] = await Promise.all([
      friendlyTry(() => actor.get_deposit_address(solPubkey)),
      friendlyTry(() => actor.get_sol_deposit_address(solPubkey)),
      friendlyTry(() => actor.get_doge_deposit_address(solPubkey)),
    ]);

    setWalletAddresses({ icp: deposit, sol: solDeposit, doge: dogeDeposit });
    setResponsiveValue("pid", "Phantom public key", solPubkey);
    setResponsiveValue("deposit", "ICP deposit address", deposit);
    setResponsiveValue("sol_deposit", "SOL deposit address", solDeposit);
    setResponsiveValue("doge_deposit", "DOGE deposit address", dogeDeposit);
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

async function refreshDogeBalance(force = false, quiet = false) {
  const now = Date.now();
  if (!force && now - lastDogeRefreshMs < refreshCooldownMs) {
    const waitSeconds = Math.ceil((refreshCooldownMs - (now - lastDogeRefreshMs)) / 1000);
    if (!quiet) {
      showWarn(`Please wait about ${waitSeconds}s before refreshing DOGE again.`);
    }
    return null;
  }
  if (dogeRefreshInFlight) {
    return null;
  }

  if (!authMode) {
    setText("doge_balance", "DOGE Balance: --");
    if (!quiet) {
      showWarn("Sign in with Internet Identity or connect Phantom before refreshing DOGE.");
    }
    return null;
  }
  if (authMode === "ii" && !identity) {
    setText("doge_balance", "DOGE Balance: --");
    if (!quiet) {
      showWarn("Sign in with Internet Identity before refreshing the II-managed DOGE balance.");
    }
    return null;
  }
  if (authMode === "phantom" && !solPubkey) {
    setText("doge_balance", "DOGE Balance: --");
    if (!quiet) {
      showWarn("Connect Phantom before refreshing the Phantom-managed DOGE balance.");
    }
    return null;
  }

  dogeRefreshInFlight = true;
  $("refresh_doge").disabled = true;

  try {
    await ensureActor();
    const satoshis =
      authMode === "ii"
        ? await withTimeout(
            friendlyTry(
              () => actor.get_doge_balance_ii(),
              (message) => !quiet && showWarn(message)
            )
          )
        : await withTimeout(
            friendlyTry(
              () => actor.get_doge_balance(solPubkey),
              (message) => !quiet && showWarn(message)
            )
          );
    setText("doge_balance", `DOGE Balance: ${(Number(satoshis) / dogeSatoshisPerDoge).toFixed(8)} DOGE`);
    lastDogeRefreshMs = Date.now();
    if (!quiet) {
      showMuted("DOGE balance updated.");
    }
    return satoshis;
  } catch (error) {
    if (!quiet) {
      showErr(normalizeAgentError(error));
    }
    return null;
  } finally {
    dogeRefreshInFlight = false;
    $("refresh_doge").disabled = false;
  }
}

async function refreshBothBalances(force = false, quiet = false) {
  await Promise.allSettled([
    refreshIcpBalance(force, quiet),
    refreshSolBalance(force, quiet),
    refreshDogeBalance(force, quiet),
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

  return success;
}

function explorerLinkForAsset(result, assetType) {
  const txidMatch = result.match(/txid (\S+)/i);
  if (!txidMatch) {
    return null;
  }

  if (assetType === "SOL") {
    return {
      href: `https://explorer.solana.com/tx/${txidMatch[1]}?cluster=mainnet-beta`,
      label: "View on Solana Explorer",
    };
  }

  if (assetType === "DOGE") {
    return {
      href: `https://live.blockcypher.com/doge/tx/${txidMatch[1]}/`,
      label: "View on BlockCypher",
    };
  }

  return null;
}

function displayTransferResult(result, assetType) {
  if (result.startsWith("Transfer successful")) {
    const explorer = explorerLinkForAsset(result, assetType);
    if (explorer) {
      renderLatestTransaction(
        result,
        "ok",
        explorer.href,
        explorer.label
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

async function copyText(buttonId, value, label) {
  if (!value) {
    showWarn(`No ${label} is available to copy yet.`);
    return;
  }

  if (copyButtonBusyIds.has(buttonId) || copyButtonFeedbackTimers.has(buttonId)) {
    return;
  }

  copyButtonBusyIds.add(buttonId);
  setButtonState(buttonId, "Copying...", { disabled: true, busy: true });

  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
    } else {
      const input = document.createElement("textarea");
      input.value = value;
      input.setAttribute("readonly", "");
      input.style.position = "absolute";
      input.style.left = "-9999px";
      document.body.appendChild(input);
      input.select();
      document.execCommand("copy");
      document.body.removeChild(input);
    }

    copyButtonBusyIds.delete(buttonId);
    const existingTimer = copyButtonFeedbackTimers.get(buttonId);
    if (existingTimer) {
      window.clearTimeout(existingTimer);
    }

    setButtonState(buttonId, "Copied", { disabled: true, busy: false });
    const restoreTimer = window.setTimeout(() => {
      copyButtonFeedbackTimers.delete(buttonId);
      updateCopyButtons();
    }, 1600);
    copyButtonFeedbackTimers.set(buttonId, restoreTimer);
    showOk(`${label} copied to clipboard.`);
  } catch (error) {
    copyButtonBusyIds.delete(buttonId);
    updateCopyButtons();
    showErr(`Failed to copy ${label}: ${normalizeAgentError(error)}`);
  }
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
  await connectPhantomWallet();
};

$("logout").onclick = async () => {
  await disconnectPhantomSession();
  await makeAgentAndActor();
  await hydrateActiveWallet(false);
  showMuted("Phantom disconnected.");
};

$("copy_icp").onclick = async () => {
  await copyText("copy_icp", icpDepositAddress, "ICP address");
};

$("copy_sol").onclick = async () => {
  await copyText("copy_sol", solDepositAddress, "SOL address");
};

$("copy_doge").onclick = async () => {
  await copyText("copy_doge", dogeDepositAddress, "DOGE address");
};

$("refresh_icp").onclick = async () => {
  await refreshIcpBalance(false, false);
};

$("get_sol").onclick = async () => {
  await refreshSolBalance(false, false);
};

$("refresh_doge").onclick = async () => {
  await refreshDogeBalance(false, false);
};

$("send").onclick = async () => {
  if (sendingIcp) {
    showWarn("An ICP transfer is already in progress.");
    return;
  }

  sendingIcp = true;
  setButtonState("send", "Sending ICP...", { disabled: true, busy: true });

  let initialNonce = null;
  let shouldClearInputs = false;

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
      shouldClearInputs = true;
      displayTransferResult(result, "ICP");
      await refreshBothBalances(true, true);
      showOk("ICP transfer submitted through Internet Identity mode.");
    } else if (authMode === "phantom") {
      if (!solPubkey) {
        throw new Error("Connect Phantom first.");
      }

      const provider = getProvider();
      if (!provider) {
        throw new Error("Open this page inside Phantom or unlock Phantom first.");
      }

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
      shouldClearInputs = true;
      displayTransferResult(result, "ICP");
      await refreshBothBalances(true, true);
      showOk("ICP transfer submitted through Phantom mode.");
    } else {
      throw new Error("Sign in with Internet Identity or connect Phantom first.");
    }

  } catch (error) {
    const message = (error?.message || String(error || "")).toLowerCase();
    if (message.includes("timed out") || message.includes("processing")) {
      shouldClearInputs = (await confirmAfterTimeout(initialNonce, "ICP")) || shouldClearInputs;
    } else if (error.message !== "Cancelled") {
      showErr(`ICP transfer failed: ${normalizeAgentError(error)}`);
    }
  } finally {
    if (shouldClearInputs) {
      clearTransferFields("to", "amount");
    }
    sendingIcp = false;
    setButtonState("send", getDefaultButtonLabel("send"), { disabled: false, busy: false });
  }
};

$("send_sol").onclick = async () => {
  if (sendingSol) {
    showWarn("A SOL transfer is already in progress.");
    return;
  }

  sendingSol = true;
  setButtonState("send_sol", "Sending SOL...", { disabled: true, busy: true });

  let initialNonce = null;
  let shouldClearInputs = false;

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
      shouldClearInputs = true;
      displayTransferResult(result, "SOL");
      await refreshBothBalances(true, true);
      showOk("SOL transfer submitted through Internet Identity mode.");
    } else if (authMode === "phantom") {
      if (!solPubkey) {
        throw new Error("Connect Phantom first.");
      }

      const provider = getProvider();
      if (!provider) {
        throw new Error("Open this page inside Phantom or unlock Phantom first.");
      }

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
      shouldClearInputs = true;
      displayTransferResult(result, "SOL");
      await refreshBothBalances(true, true);
      showOk("SOL transfer submitted through Phantom mode.");
    } else {
      throw new Error("Sign in with Internet Identity or connect Phantom first.");
    }

  } catch (error) {
    const message = (error?.message || String(error || "")).toLowerCase();
    if (message.includes("timed out") || message.includes("processing")) {
      shouldClearInputs = (await confirmAfterTimeout(initialNonce, "SOL")) || shouldClearInputs;
    } else if (error.message !== "Cancelled") {
      showErr(`SOL transfer failed: ${normalizeAgentError(error)}`);
    }
  } finally {
    if (shouldClearInputs) {
      clearTransferFields("to_sol", "amount_sol");
    }
    sendingSol = false;
    setButtonState("send_sol", getDefaultButtonLabel("send_sol"), {
      disabled: false,
      busy: false,
    });
  }
};

$("send_doge").onclick = async () => {
  if (sendingDoge) {
    showWarn("A DOGE transfer is already in progress.");
    return;
  }

  sendingDoge = true;
  setButtonState("send_doge", "Sending DOGE...", { disabled: true, busy: true });

  let initialNonce = null;
  let shouldClearInputs = false;

  try {
    const to = $("to_doge").value.trim();
    const amountInput = $("amount_doge").value.trim();
    const amountDoge = validatePositiveAmount(amountInput, "DOGE");
    const amountSats = BigInt(Math.round(amountDoge * dogeSatoshisPerDoge));

    if (!to) {
      throw new Error("Enter a destination Dogecoin address.");
    }

    await ensureActor();

    if (authMode === "ii") {
      if (!identity) {
        throw new Error("Sign in with Internet Identity first.");
      }

      initialNonce = await actor.get_nonce_ii();
      const confirmed = window.confirm(
        `Send ${amountDoge.toFixed(8)} DOGE?\n\nDestination: ${to}\nNetwork fee: provider-selected at submission time\nNotes: DOGE miner fees vary with the UTXOs currently available in this wallet.`
      );
      if (!confirmed) {
        throw new Error("Cancelled");
      }

      const result = await withTimeout(
        friendlyTry(
          () => actor.transfer_doge_ii(to, amountSats),
          (message) => showWarn(message)
        )
      );
      shouldClearInputs = true;
      displayTransferResult(result, "DOGE");
      await refreshBothBalances(true, true);
      showOk("DOGE transfer submitted through Internet Identity mode.");
    } else if (authMode === "phantom") {
      if (!solPubkey) {
        throw new Error("Connect Phantom first.");
      }

      const provider = getProvider();
      if (!provider) {
        throw new Error("Open this page inside Phantom or unlock Phantom first.");
      }

      initialNonce = await actor.get_nonce(solPubkey);
      const confirmed = window.confirm(
        `Send ${amountDoge.toFixed(8)} DOGE?\n\nDestination: ${to}\nNetwork fee: provider-selected at submission time\nNotes: DOGE miner fees vary with the UTXOs currently available in this wallet.`
      );
      if (!confirmed) {
        throw new Error("Cancelled");
      }

      const message = `transfer_doge to ${to} amount ${amountSats} nonce ${initialNonce}`;
      const signed = await provider.signMessage(textEncoder.encode(message), "utf8");
      const result = await withTimeout(
        friendlyTry(
          () =>
            actor.transfer_doge(
              to,
              amountSats,
              solPubkey,
              Array.from(signed.signature),
              initialNonce
            ),
          (errorMessage) => showWarn(errorMessage)
        )
      );
      shouldClearInputs = true;
      displayTransferResult(result, "DOGE");
      await refreshBothBalances(true, true);
      showOk("DOGE transfer submitted through Phantom mode.");
    } else {
      throw new Error("Sign in with Internet Identity or connect Phantom first.");
    }

  } catch (error) {
    const message = (error?.message || String(error || "")).toLowerCase();
    if (message.includes("timed out") || message.includes("processing")) {
      shouldClearInputs = (await confirmAfterTimeout(initialNonce, "DOGE")) || shouldClearInputs;
    } else if (error.message !== "Cancelled") {
      showErr(`DOGE transfer failed: ${normalizeAgentError(error)}`);
    }
  } finally {
    if (shouldClearInputs) {
      clearTransferFields("to_doge", "amount_doge");
    }
    sendingDoge = false;
    setButtonState("send_doge", getDefaultButtonLabel("send_doge"), {
      disabled: false,
      busy: false,
    });
  }
};

await restoreIiSession();
await restoreTrustedPhantomConnection();
await makeAgentAndActor();
bindProviderEvents();
renderConnectionState();
updateCopyButtons();
resetLatestTransaction();
if (compactValueMediaQuery.addEventListener) {
  compactValueMediaQuery.addEventListener("change", renderResponsiveValues);
} else if (compactValueMediaQuery.addListener) {
  compactValueMediaQuery.addListener(renderResponsiveValues);
}
const resumedPhantomMobileConnect = await maybeContinuePhantomMobileConnect();
if (!resumedPhantomMobileConnect.connected) {
  await hydrateActiveWallet(Boolean(identity || solPubkey));
}
if (!resumedPhantomMobileConnect.attempted) {
  showMuted("Ready with ICP, SOL, and DOGE.");
}
