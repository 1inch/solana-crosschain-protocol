import {
  Connection,
  Keypair,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { Program } from "@coral-xyz/anchor";

import WHITELIST_IDL from "../../target/idl/whitelist.json";
import { Whitelist } from "../../target/types/whitelist";

import {
  findWhitelistStateAddress,
  connectTo,
  loadKeypairFromFile,
  prompt_,
} from "../utils";

async function initialize(
  connection: Connection,
  program: Program<Whitelist>,
  authorityKeypair: Keypair
): Promise<void> {
  const whitelistState = findWhitelistStateAddress(program.programId);

  const initializeIx = await program.methods
    .initialize()
    .accountsPartial({
      authority: authorityKeypair.publicKey,
      whitelistState,
    })
    .signers([authorityKeypair])
    .instruction();

  const tx = new Transaction().add(initializeIx);

  const signature = await sendAndConfirmTransaction(connection, tx, [
    authorityKeypair,
  ]);
  console.log(`Transaction signature ${signature}`);
}

async function main() {
  const {connection, program: whitelist} = await connectTo<Whitelist>(WHITELIST_IDL as any);

  const authorityKeypairPath = prompt_(
    "authority-kp",
    "Enter authority keypair path: "
  );
  const authorityKeypair = await loadKeypairFromFile(authorityKeypairPath);
  if (!authorityKeypair) {
    throw new Error("Failed to load authority keypair.");
  }
  await initialize(connection, whitelist, authorityKeypair);
}

main();
