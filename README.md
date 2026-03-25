# IC Phantom

IC Phantom is an Internet Computer asset-canister frontend and Rust backend that combines Internet Identity and Phantom-driven wallet flows in one workspace for ICP, SOL, and DOGE.

## Frontend branding

The static frontend lives in `src/sol_icp_poc_frontend/assets` and now includes:

- IC Phantom Open Graph and Twitter preview metadata
- Favicon, Apple touch icon, and web manifest entries
- A dark neon visual theme that matches the IC Phantom preview artwork

## Running locally

Start the local replica and deploy the canisters:

```bash
dfx start --background
dfx deploy
```

The frontend asset canister will be available through the local DFX gateway after deploy.
