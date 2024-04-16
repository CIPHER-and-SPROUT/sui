// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { ReplayLink } from '@/components/replay/ReplayLink';

import { PreviewCard } from '../preview-effects/PreviewCard';
import { ChangedObjects, EffectsV2, UnchangedSharedObjects } from './replay-types';

export function ReplayEffects({ effects }: { effects: EffectsV2 }) {
	const output = [];

	if (effects.changedObjects) {
		output.push(effectsSectionChangedObjects(effects.changedObjects));
	}
	if (effects.unchangedSharedObjects) {
		output.push(effectsSectionUnchangedSharedObjects(effects.unchangedSharedObjects));
	}

	output.push(
		<div>
			<PreviewCard.Root className="m-2">
				<PreviewCard.Header> Dependencies </PreviewCard.Header>
				<PreviewCard.Body>
					<div className="text-sm max-h-[450px] overflow-y-auto grid grid-cols-1 gap-3">
						{effects.dependencies.map((dep) => (
							<ReplayLink landing={true} id={dep} text={dep} />
						))}
					</div>
				</PreviewCard.Body>
			</PreviewCard.Root>
		</div>,
	);
	return output;
}

const effectsSectionChangedObjects = (input: ChangedObjects) => {
	return (
		<div>
			<PreviewCard.Root className="m-2">
				<PreviewCard.Header> Changed Objects </PreviewCard.Header>
				<PreviewCard.Body>
					<div className="text-sm max-h-[450px] overflow-y-auto grid grid-cols-1 gap-3">
						{input.map((item, index) => (
							<>
								<ReplayLink landing={true} text={item[0]} />
							</>
						))}
					</div>
				</PreviewCard.Body>
			</PreviewCard.Root>
		</div>
	);
};

const effectsSectionUnchangedSharedObjects = (input: UnchangedSharedObjects) => {
	return (
		<div>
			<PreviewCard.Root className="m-2">
				<PreviewCard.Header> Unchanged Shared Objects </PreviewCard.Header>
				<PreviewCard.Body>
					<div className="text-sm max-h-[450px] overflow-y-auto grid grid-cols-1 gap-3">
						{input.map((item, index) => (
							<>
								<ReplayLink landing={true} text={item[0]} />
							</>
						))}
					</div>
				</PreviewCard.Body>
			</PreviewCard.Root>
		</div>
	);
};
