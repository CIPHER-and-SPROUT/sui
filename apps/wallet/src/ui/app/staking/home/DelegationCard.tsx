// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useFormatCoin } from '@mysten/core';
import { SUI_TYPE_ARG, type SuiAddress } from '@mysten/sui.js';
import { Link } from 'react-router-dom';

import { ValidatorLogo } from '../validators/ValidatorLogo';
import { Text } from '_src/ui/app/shared/text';
import { IconTooltip } from '_src/ui/app/shared/tooltip';

import type { StakeObject } from '@mysten/sui.js';

export enum DelegationState {
    EARNING = 'EARNING',
    COOL_DOWN = 'COOL_DOWN',
    WITH_DRAW = 'WITH_DRAW',
    IN_ACTIVE = 'IN_ACTIVE',
}

interface DelegationObjectWithValidator extends StakeObject {
    validatorAddress: SuiAddress;
}
interface DelegationCardProps {
    delegationObject: DelegationObjectWithValidator;
    currentEpoch: number;
}

// For delegationsRequestEpoch n  through n + 2, show Start Earning
// Show epoch number or date/time for n + 3 epochs
// TODO: Add cool-down state
export function DelegationCard({
    delegationObject,
    currentEpoch,
}: DelegationCardProps) {
    const {
        stakedSuiId,
        principal,
        stakeRequestEpoch,
        estimatedReward,
        validatorAddress,
    } = delegationObject;
    const rewards = estimatedReward;

    const numberOfEpochPastRequesting = currentEpoch - stakeRequestEpoch;
    const [stakedFormatted] = useFormatCoin(principal, SUI_TYPE_ARG);
    const [rewardsFormatted] = useFormatCoin(rewards, SUI_TYPE_ARG);

    return (
        <Link
            to={`/stake/delegation-detail?${new URLSearchParams({
                validator: validatorAddress,
                staked: stakedSuiId,
            }).toString()}`}
            className="no-underline"
        >
            <div className="flex justify-between items-start mb-1">
                <ValidatorLogo
                    validatorAddress={validatorAddress}
                    size="subtitle"
                    iconSize="md"
                    stacked
                />

                <div className="text-gray-60 text-p1 opacity-0 group-hover:opacity-100">
                    <IconTooltip
                        tip="Object containing the delegated staked SUI tokens, owned by each delegator"
                        placement="top"
                    />

            <div className="flex-1 mb-4">
                <div className="flex items-baseline gap-1">
                    <Text variant="body" weight="semibold" color="gray-90">
                        {stakedFormatted}
                    </Text>

                    <Text variant="subtitle" weight="normal" color="gray-90">
                        SUI
                    </Text>
                </div>
            </div>
            <div className="flex flex-col gap-1">
                <Text variant="subtitle" weight="medium" color="steel-dark">
                    {numberOfEpochPastRequesting > 2
                        ? 'Staking Reward'
                        : 'Starts Earning'}
                </Text>
                {numberOfEpochPastRequesting <= 2 && (
                    <Text
                        variant="subtitle"
                        weight="semibold"
                        color="steel-dark"
                    >
                        Epoch #{stakeRequestEpoch + 2}
                    </Text>
                )}

                {rewards && rewards > 0 && numberOfEpochPastRequesting > 2 ? (
                    <div className="text-success-dark text-bodySmall font-semibold">
                        {rewardsFormatted} SUI
                    </div>
                ) : null}
            </div>
        </Link>
    );
}
