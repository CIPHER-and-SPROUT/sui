// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  array,
  boolean,
  literal,
  number,
  object,
  string,
  union,
  Infer,
  nullable,
  tuple,
  optional,
} from 'superstruct';
import { SuiAddress } from './common';
import { AuthorityName } from './transactions';

/* -------------- Types for the SuiSystemState Rust definition -------------- */

export const ValidatorMetaData = object({
  suiAddress: SuiAddress,
  protocolPubkeyBytes: array(number()),
  networkPubkeyBytes: array(number()),
  workerPubkeyBytes: array(number()),
  proofOfPossessionBytes: array(number()),
  name: string(),
  description: string(),
  imageUrl: string(),
  projectUrl: string(),
  p2pAddress: array(number()),
  netAddress: array(number()),
  primaryAddress: array(number()),
  workerAddress: array(number()),
  nextEpochProtocolPubkeyBytes: nullable(array(number())),
  nextEpochProofOfPossession: nullable(array(number())),
  nextEpochNetworkPubkeyBytes: nullable(array(number())),
  nextEpochWorkerPubkeyBytes: nullable(array(number())),
  nextEpochNetAddress: nullable(array(number())),
  nextEpochP2pAddress: nullable(array(number())),
  nextEpochPrimaryAddress: nullable(array(number())),
  nextEpochWorkerAddress: nullable(array(number())),
});

export type DelegatedStake = Infer<typeof DelegatedStake>;
export type ValidatorMetaData = Infer<typeof ValidatorMetaData>;
export type CommitteeInfo = Infer<typeof CommitteeInfo>;

// Staking

export const Balance = object({
  value: number(),
});

export const StakedSui = object({
  id: object({
    id: string(),
  }),
  poolId: string(),
  validatorAddress: string(),
  delegationRequestEpoch: number(),
  principal: Balance,
  suiTokenLock: union([number(), literal(null)]),
});

export const ActiveFields = object({
  id: object({
    id: string(),
  }),
  stakedSuiId: SuiAddress,
  principalSuiAmount: number(),
  poolTokens: Balance,
});

export const ActiveDelegationStatus = object({
  Active: ActiveFields,
});

export const DelegatedStake = object({
  stakedSui: StakedSui,
  delegationStatus: union([literal('Pending'), ActiveDelegationStatus]),
});

export const ParametersFields = object({
  max_validator_count: string(),
  min_validator_stake: string(),
  storage_gas_price: optional(string()),
});

export const Parameters = object({
  type: string(),
  fields: ParametersFields,
});

export const StakeSubsidyFields = object({
  balance: object({ value: number() }),
  currentEpochAmount: number(),
  epochCounter: number(),
});

export const StakeSubsidy = object({
  type: string(),
  fields: StakeSubsidyFields,
});

export const SuiSupplyFields = object({
  value: number(),
});

export const ContentsFields = object({
  id: string(),
  size: number(),
  head: object({ vec: array() }),
  tail: object({ vec: array() }),
});

export const ContentsFieldsWithdraw = object({
  id: string(),
  size: number(),
});

export const Contents = object({
  type: string(),
  fields: ContentsFields,
});

export const DelegationStakingPoolFields = object({
  exchangeRates: object({
    id: string(),
    size: number(),
  }),
  id: string(),
  pendingDelegation: number(),
  pendingPoolTokenWithdraw: number(),
  pendingTotalSuiWithdraw: number(),
  poolTokenBalance: number(),
  rewardsPool: object({ value: number() }),
  activationEpoch: object({ vec: array(number()) }),
  deactivationEpoch: object({ vec: array() }),
  suiBalance: number(),
});

export const DelegationStakingPool = object({
  type: string(),
  fields: DelegationStakingPoolFields,
});

export const CommitteeInfo = object({
  epoch: number(),
  /** Array of (validator public key, stake unit) tuple */
  validators: optional(array(tuple([AuthorityName, number()]))),
});

export const SystemParameters = object({
  minValidatorStake: number(),
  maxValidatorCount: number(),
  governanceStartEpoch: number(),
  storageGasPrice: optional(number()),
});

export const Validator = object({
  metadata: ValidatorMetaData,
  votingPower: number(),
  gasPrice: number(),
  stakingPool: DelegationStakingPoolFields,
  commissionRate: number(),
  nextEpochStake: number(),
  nextEpochGasPrice: number(),
  nextEpochCommissionRate: number(),
});
export type Validator = Infer<typeof Validator>;

export const ValidatorPair = object({
  from: SuiAddress,
  to: SuiAddress,
});

export const ValidatorSet = object({
  totalStake: number(),
  activeValidators: array(Validator),
  pendingActiveValidators: object({
    contents: object({
      id: string(),
      size: number(),
    }),
  }),
  pendingRemovals: array(number()),
  stakingPoolMappings: object({
    id: string(),
    size: number(),
  }),
  inactivePools: object({
    id: string(),
    size: number(),
  }),
  validatorCandidates: object({
    id: string(),
    size: number(),
  }),
});

export const SuiSystemState = object({
  epoch: number(),
  protocolVersion: number(),
  validators: ValidatorSet,
  storageFund: Balance,
  parameters: SystemParameters,
  referenceGasPrice: number(),
  validatorReportRecords: object({ contents: array() }),
  stakeSubsidy: StakeSubsidyFields,
  safeMode: boolean(),
  epochStartTimestampMs: optional(number()),
});

export type SuiSystemState = Infer<typeof SuiSystemState>;

export const SuiValidatorSummary = object({
  suiAddress: SuiAddress,
  protocolPubkeyBytes: array(number()),
  networkPubkeyBytes: array(number()),
  workerPubkeyBytes: array(number()),
  proofOfPossessionBytes: array(number()),
  name: string(),
  description: string(),
  imageUrl: string(),
  projectUrl: string(),
  p2pAddress: array(number()),
  netAddress: array(number()),
  primaryAddress: array(number()),
  workerAddress: array(number()),
  nextEpochProtocolPubkeyBytes: nullable(array(number())),
  nextEpochProofOfPossession: nullable(array(number())),
  nextEpochNetworkPubkeyBytes: nullable(array(number())),
  nextEpochWorkerPubkeyBytes: nullable(array(number())),
  nextEpochNetAddress: nullable(array(number())),
  nextEpochP2pAddress: nullable(array(number())),
  nextEpochPrimaryAddress: nullable(array(number())),
  nextEpochWorkerAddress: nullable(array(number())),
  votingPower: number(),
  gasPrice: number(),
  commissionRate: number(),
  nextEpochStake: number(),
  nextEpochGasPrice: number(),
  nextEpochCommissionRate: number(),
  stakingPoolId: string(),
  stakingPoolActivationEpoch: nullable(number()),
  stakingPoolDeactivationEpoch: nullable(number()),
  stakingPoolSuiBalance: number(),
  rewardsPool: number(),
  poolTokenBalance: number(),
  pendingDelegation: number(),
  pendingPoolTokenWithdraw: number(),
  pendingTotalSuiWithdraw: number(),
});

export type SuiValidatorSummary = Infer<typeof SuiValidatorSummary>;

export const SuiSystemStateSummary = object({
  epoch: number(),
  protocolVersion: number(),
  storageFund: number(),
  referenceGasPrice: number(),
  safeMode: boolean(),
  epochStartTimestampMs: number(),
  minValidatorStake: number(),
  maxValidatorCandidateCount: number(),
  governanceStartEpoch: number(),
  stakeSubsidyEpochCounter: number(),
  stakeSubsidyBalance: number(),
  stakeSubsidyCurrentEpochAmount: number(),
  totalStake: number(),
  activeValidators: array(SuiValidatorSummary),
});

export type SuiSystemStateSummary = Infer<typeof SuiSystemStateSummary>;
