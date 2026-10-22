/** Runtime configuration for the compliance event indexer. */
export interface Config {
  rpcUrl: string;
  networkPassphrase: string;
  allowlistContractId: string;
  denylistContractId: string;
  jurisdictionContractId: string;
  /** Contract ID of the deployed multisig-admin contract (or empty to skip) */
  multisigContractId: string;
  /** Contract ID of the deployed compliance-aggregator contract (or empty to skip) */
  aggregatorContractId: string;
  /** Contract ID of the deployed policy-engine contract (or empty to skip) */
  policyEngineContractId: string;
  /** Contract ID of the deployed circuit-breaker contract (or empty to skip) */
  circuitBreakerContractId: string;
  /** Contract ID of the deployed audit-log contract (or empty to skip) */
  auditLogContractId: string;
  dbPath: string;
  pollIntervalMs: number;
  startLedger: number;
  /** Stop after indexing this ledger; 0 keeps polling indefinitely. */
  endLedger: number;
}

const CONTRACT_ID = /^C[A-Z2-7]{55}$/;

const CONTRACT_ID_ENV_VARS = [
  "ALLOWLIST_CONTRACT_ID",
  "DENYLIST_CONTRACT_ID",
  "JURISDICTION_CONTRACT_ID",
  "MULTISIG_CONTRACT_ID",
  "AGGREGATOR_CONTRACT_ID",
  "POLICY_ENGINE_CONTRACT_ID",
  "CIRCUIT_BREAKER_CONTRACT_ID",
  "AUDIT_LOG_CONTRACT_ID",
] as const;

function required(name: string, env: NodeJS.ProcessEnv): string {
  const value = env[name]?.trim();
  if (!value) throw new Error(`Invalid indexer configuration: ${name} is required`);
  return value;
}

function contractId(name: string, env: NodeJS.ProcessEnv): string {
  const value = required(name, env);
  if (!CONTRACT_ID.test(value)) {
    throw new Error(`Invalid indexer configuration: ${name} must be a valid Soroban contract ID`);
  }
  return value;
}

function positiveInteger(name: string, raw: string | undefined, fallback: number, allowZero = false): number {
  const value = raw === undefined ? fallback : Number(raw);
  const valid = Number.isInteger(value) && (allowZero ? value >= 0 : value > 0);
  if (!valid) throw new Error(`Invalid indexer configuration: ${name} must be an ${allowZero ? "integer >= 0" : "integer > 0"}`);
  return value;
}

export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  const rpcUrl = required("RPC_URL", env);
  try {
    const parsed = new URL(rpcUrl);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") throw new Error();
  } catch {
    throw new Error("Invalid indexer configuration: RPC_URL must be an absolute HTTP(S) URL");
  }

  const configuredContracts = CONTRACT_ID_ENV_VARS.filter((name) => env[name]?.trim());
  if (configuredContracts.length === 0) throw new Error("Invalid indexer configuration: at least one contract ID is required");
  for (const name of configuredContracts) contractId(name, env);

  const startLedger = positiveInteger("START_LEDGER", env.START_LEDGER, 0, true);
  const endLedger = positiveInteger("END_LEDGER", env.END_LEDGER, 0, true);
  if (startLedger > 0 && endLedger > 0 && endLedger < startLedger) {
    throw new Error("Invalid indexer configuration: END_LEDGER must be >= START_LEDGER");
  }

  return {
    rpcUrl,
    networkPassphrase: env.NETWORK_PASSPHRASE?.trim() || "Test SDF Network ; September 2015",
    allowlistContractId: env.ALLOWLIST_CONTRACT_ID?.trim() || "",
    denylistContractId: env.DENYLIST_CONTRACT_ID?.trim() || "",
    jurisdictionContractId: env.JURISDICTION_CONTRACT_ID?.trim() || "",
    multisigContractId: env.MULTISIG_CONTRACT_ID?.trim() || "",
    aggregatorContractId: env.AGGREGATOR_CONTRACT_ID?.trim() || "",
    policyEngineContractId: env.POLICY_ENGINE_CONTRACT_ID?.trim() || "",
    circuitBreakerContractId: env.CIRCUIT_BREAKER_CONTRACT_ID?.trim() || "",
    auditLogContractId: env.AUDIT_LOG_CONTRACT_ID?.trim() || "",
    dbPath: required("DB_PATH", env),
    pollIntervalMs: positiveInteger("POLL_INTERVAL_MS", env.POLL_INTERVAL_MS, 5000),
    startLedger,
    endLedger,
  };
}
