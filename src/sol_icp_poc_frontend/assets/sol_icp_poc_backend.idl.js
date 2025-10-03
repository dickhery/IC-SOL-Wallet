// src/sol_icp_poc_frontend/assets/sol_icp_poc_backend.idl.js
import { IDL } from "@dfinity/candid";

export const idlFactory = ({ IDL }) =>
  IDL.Service({
    whoami: IDL.Func([], [IDL.Text], ['query']),
    link_sol_pubkey: IDL.Func([IDL.Text, IDL.Vec(IDL.Nat8)], [IDL.Text], []),
    unlink_sol_pubkey: IDL.Func([], [IDL.Text], []),

    get_deposit_address: IDL.Func([IDL.Text], [IDL.Text], ['query']),
    get_balance: IDL.Func([IDL.Text], [IDL.Nat64], []),
    get_nonce: IDL.Func([IDL.Text], [IDL.Nat64], []), // update (no 'query')
    get_pid: IDL.Func([IDL.Text], [IDL.Text], ['query']),
    get_sol_deposit_address: IDL.Func([IDL.Text], [IDL.Text], []),
    get_sol_balance: IDL.Func([IDL.Text], [IDL.Nat64], []),

    transfer: IDL.Func(
      [IDL.Text, IDL.Nat64, IDL.Text, IDL.Vec(IDL.Nat8), IDL.Nat64],
      [IDL.Text],
      []
    ),
    transfer_sol: IDL.Func(
      [IDL.Text, IDL.Nat64, IDL.Text, IDL.Vec(IDL.Nat8), IDL.Nat64],
      [IDL.Text],
      []
    ),

    get_sol_deposit_address_ii: IDL.Func([], [IDL.Text], []),
    get_deposit_address_ii: IDL.Func([], [IDL.Text], []),
    get_sol_balance_ii: IDL.Func([], [IDL.Nat64], []),
    get_balance_ii: IDL.Func([], [IDL.Nat64], []),
    get_nonce_ii: IDL.Func([], [IDL.Nat64], []),
    transfer_ii: IDL.Func([IDL.Text, IDL.Nat64], [IDL.Text], []),
    transfer_sol_ii: IDL.Func([IDL.Text, IDL.Nat64], [IDL.Text], []),
  });

export default idlFactory;
