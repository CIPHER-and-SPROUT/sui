// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect, beforeEach } from 'vitest';

import { TestToolbox, setupSuiClient, setupPool } from './setup';

describe('Create a pool', () => {
	let toolbox: TestToolbox;

	beforeEach(async () => {
		toolbox = await setupSuiClient();
	});

	it('test creating a pool', async () => {
		const pool = await setupPool(toolbox);
		expect(pool.poolId).toBeDefined();
	});
});
