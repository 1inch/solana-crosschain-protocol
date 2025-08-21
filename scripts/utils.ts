import { PublicKey, Keypair, Connection } from "@solana/web3.js";
import * as anchor from "@coral-xyz/anchor";
import fs from "fs";
import path from "path";
import os from "os";
import yargs from "yargs";
import { hideBin } from "yargs/helpers";
const prompt = require("prompt-sync")({ sigint: true });

export async function connectTo<T extends anchor.Idl>(idl: T) {
  const clusterUrl = getClusterUrlEnv();
  const connection = new Connection(clusterUrl, "confirmed");
  return {
    connection,
    program: new anchor.Program<T>(idl, { connection })
  };
}

export async function loadKeypairFromFile(
  filePath: string
): Promise<Keypair | undefined> {
  // This is here so you can also load the default keypair from the file system.
  const resolvedPath = path.resolve(
    filePath.startsWith("~") ? filePath.replace("~", os.homedir()) : filePath
  );

  try {
    const raw = fs.readFileSync(resolvedPath);
    const formattedData = JSON.parse(raw.toString());

    const keypair = Keypair.fromSecretKey(Uint8Array.from(formattedData));
    return keypair;
  } catch (error) {
    throw new Error(
      `Error reading keypair from file: ${(error as Error).message}`
    );
  }
}

export function findResolverAccessAddress(
  programId: PublicKey,
  user: PublicKey
): PublicKey {
  const [resolverAccess] = PublicKey.findProgramAddressSync(
    [anchor.utils.bytes.utf8.encode("resolver_access"), user.toBuffer()],
    programId
  );

  return resolverAccess;
}

export function findWhitelistStateAddress(programId: PublicKey): PublicKey {
  const [whitelistState] = PublicKey.findProgramAddressSync(
    [anchor.utils.bytes.utf8.encode("whitelist_state")],
    programId
  );

  return whitelistState;
}

export function getClusterUrlEnv() {
  const clusterUrl = process.env.CLUSTER_URL;
  if (!clusterUrl) {
    throw new Error("Missing CLUSTER_URL environment variable");
  }
  return clusterUrl;
}

// return argument if provided in cmd line, else ask the user and get it.
export function prompt_(key: string, pmpt: string): string {
  const argv = yargs(hideBin(process.argv)).parse();
  if (key in argv) {
    return argv[key];
  } else {
    return prompt(pmpt);
  }
}
