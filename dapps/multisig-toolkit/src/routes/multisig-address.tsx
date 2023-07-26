// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import {
	PubkeyWeightPair,
	publicKeyFromSerialized,
	SIGNATURE_FLAG_TO_SCHEME,
	toB64,
	toMultiSigAddress,
	SignatureScheme,
	fromB64,
} from '@mysten/sui.js';
import { AlertCircle } from 'lucide-react';
import { useState } from 'react';
import { Label } from '@/components/ui/label';
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from '@/components/ui/card';

import { useForm, useFieldArray, Controller, useWatch, FieldValues } from 'react-hook-form';

import { z } from 'zod';
import { zodResolver } from '@hookform/resolvers/zod';

/*
Pubkeys for playing with
ABr818VXt+6PLPRoA7QnsHBfRpKJdWZPjt7ppiTl6Fkq
ANRdB4M6Hj73R+gRM4N6zUPNidLuatB9uccOzHBc/0bP
*/

const schema = z.object({
	threshold: z
		.number({ invalid_type_error: 'Age field is required.' })
		.min(1, { message: 'threshold must be at least 3 characters.' }),
});

type FormData = z.infer<typeof schema>;

let renderCount = 0;

//function MultiSigAddress({ signature, index }: ) {
//	const suiAddress = signature.publicKey.toSuiAddress();
//
//	const pubkey_base64_sui_format = signature.publicKey.toSuiPublicKey();
//
//	const pubkey = signature.publicKey.toBase64();
//	const scheme = signature.signatureScheme.toString();
//
//	const details = [
//		{ label: 'Signature Public Key', value: pubkey },
//		{ label: 'Sui Format Public Key ( flag | pk )', value: pubkey_base64_sui_format },
//		{ label: 'Sui Address', value: suiAddress },
//		{ label: 'Signature', value: toB64(signature.signature) },
//	];
//
//	return (
//		<Card>
//			<CardHeader>
//				<CardTitle>Signature #{index}</CardTitle>
//				<CardDescription>{scheme}</CardDescription>
//			</CardHeader>
//			<CardContent>
//				<div className="flex flex-col gap-2">
//					{details.map(({ label, value }, index) => (
//						<div key={index} className="flex flex-col gap-1.5">
//							<div className="font-bold">{label}</div>
//							<div className="bg-muted rounded text-sm font-mono p-2 break-all">{value}</div>
//						</div>
//					))}
//				</div>
//			</CardContent>
//		</Card>
//	);
//}

export default function MultiSigAddressGenerator() {
	const [msAddress, setMSAddress] = useState('');
	const { register, control, handleSubmit } = useForm({
		defaultValues: {
			pubKeys: [{ pubKey: 'Sui Pubkey', weight: '' }],
			threshold: 1,
		},
	});
	const { fields, append, remove } = useFieldArray({
		control,
		name: 'pubKeys',
	});

	// Perform generation of multisig address
	const onSubmit = (data: FieldValues) => {
		console.log('data', data);

		let pks: PubkeyWeightPair[] = [];
		data.pubKeys.forEach((item: any) => {
			console.log(item.pubKey);
			const pkBytes = fromB64(item.pubKey);
			const flag: number = pkBytes[0];
			console.log(flag);
			const rawPkBytes = toB64(pkBytes.slice(1));
			const schemeFlag = (SIGNATURE_FLAG_TO_SCHEME as { [key: number]: string })[flag];
			const scheme = schemeFlag as SignatureScheme;

			const pk = publicKeyFromSerialized(scheme, rawPkBytes);
			console.log(pk);
			pks.push({ pubKey: pk, weight: item.weight });
		});
		console.log('pks:', pks);
		const multisigSuiAddress = toMultiSigAddress(pks, data.threshold);
		console.log('multisigSuiAddress', multisigSuiAddress);
		setMSAddress(multisigSuiAddress);
	};

	// if you want to control your fields with watch
	// const watchResult = watch("pubKeys");
	// console.log(watchResult);

	// The following is useWatch example
	// console.log(useWatch({ name: "pubKeys", control }));

	renderCount++;

	return (
		<div className="flex flex-col gap-4">
			<h2 className="scroll-m-20 text-4xl font-extrabold tracking-tight lg:text-5xl">
				MultiSig Address Creator
			</h2>

			<form className="flex flex-col gap-4" onSubmit={handleSubmit(onSubmit)}>
				<p>The following demo allow you to create Sui MultiSig addresses.</p>
				<ul className="grid w-full gap-1.5">
					{fields.map((item, index) => {
						return (
							<li key={item.id}>
								<input
									className="min-h-[80px] rounded-md border border-input bg-transparent px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
									{...register(`pubKeys.${index}.pubKey`, { required: true })}
								/>

								<input
									className="min-h-[80px] rounded-md border border-input bg-transparent px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
									type="number"
									{...register(`pubKeys.${index}.weight`, { required: true })}
								/>

								{/* <Controller
									render={({ field }) => (
										<input
											className="min-h-[80px] rounded-md border border-input bg-transparent px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
											{...field}
										/>
									)}
									name={`pubKeys.${index}.weight`}
									control={control}
								/> */}
								<Button
									className="min-h-[80px] rounded-md border border-input px-3 py-2 text-sm padding-2"
									type="button"
									onClick={() => remove(index)}
								>
									Delete
								</Button>
							</li>
						);
					})}
				</ul>
				<section>
					<Button
						type="button"
						onClick={() => {
							append({ pubKey: 'Sui Pubkey', weight: '' });
						}}
					>
						New PubKey
					</Button>
				</section>
				<section>
					<label className="form-label min-h-[80px] rounded-md border text-sm px-3 py-2 ring-offset-background">
						MultiSig Threshold Value:
					</label>
					<input
						className="min-h-[80px] rounded-md border border-input bg-transparent px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
						type="number"
						{...register(`threshold`, { valueAsNumber: true, required: true })}
					/>
				</section>

				{/* <input
					{...register('threshold', { valueAsNumber: true })}
					id="threshold"
					type="number"
					className="form-control"
				/> */}

				<Button
					type="submit"
					onClick={() => {
						console.log('fields', fields);
					}}
				>
					Submit
				</Button>
			</form>
			{msAddress && <Card key={msAddress}>
				<CardHeader>
					<CardTitle>Sui MultiSig Address</CardTitle>
					<CardDescription>https://docs.sui.io/testnet/learn/cryptography/sui-multisig</CardDescription>
				</CardHeader>
				<CardContent>
					<div className="flex flex-col gap-2">
						<div className="bg-muted rounded text-sm font-mono p-2 break-all">{msAddress}</div>
					</div>
				</CardContent>
			</Card>}
		</div>
	);
}

/* 
➜  multisig-toolkit git:(jnaulty/multisig-create-address) ✗ sui keytool multi-sig-address --pks ABr818VXt+6PLPRoA7QnsHBfRpKJdWZPjt7ppiTl6Fkq ANRdB4M6Hj73R+gRM4N6zUPNidLuatB9uccOzHBc/0bP --weights 1 2 --threshold 2
MultiSig address: 0x27b17213bc702893bb3e92ba84071589a6331f35f066ad15b666b9527a288c16
Participating parties:
                Sui Address                 |                Public Key (Base64)                 | Weight
----------------------------------------------------------------------------------------------------
 0x504f656b7bc467f6eb1d05dc26447477921f05e5ea88c5715682ad28835268ce | ABr818VXt+6PLPRoA7QnsHBfRpKJdWZPjt7ppiTl6Fkq  |   1   
 0x611f6a023c5d1c98b4de96e9da64daffaeb372fed0176536168908e50f6e07c0 | ANRdB4M6Hj73R+gRM4N6zUPNidLuatB9uccOzHBc/0bP  |   2   
➜  multisig-toolkit git:(jnaulty/multisig-create-address) ✗ sui keytool multi-sig-address --pks ABr818VXt+6PLPRoA7QnsHBfRpKJdWZPjt7ppiTl6Fkq ANRdB4M6Hj73R+gRM4N6zUPNidLuatB9uccOzHBc/0bP --weights 1 1 --threshold 2
MultiSig address: 0x9134bd58a25a6b48811d1c65770dd1d01e113931ed35c13f1a3c26ed7eccf9bc
Participating parties:
                Sui Address                 |                Public Key (Base64)                 | Weight
----------------------------------------------------------------------------------------------------
 0x504f656b7bc467f6eb1d05dc26447477921f05e5ea88c5715682ad28835268ce | ABr818VXt+6PLPRoA7QnsHBfRpKJdWZPjt7ppiTl6Fkq  |   1   
 0x611f6a023c5d1c98b4de96e9da64daffaeb372fed0176536168908e50f6e07c0 | ANRdB4M6Hj73R+gRM4N6zUPNidLuatB9uccOzHBc/0bP  |   1   
➜  multisig-toolkit git:(jnaulty/multisig-create-address) ✗ sui keytool multi-sig-address --pks ABr818VXt+6PLPRoA7QnsHBfRpKJdWZPjt7ppiTl6Fkq ANRdB4M6Hj73R+gRM4N6zUPNidLuatB9uccOzHBc/0bP --weights 1 1 --threshold 1
MultiSig address: 0xda3f8c1ba647d63b89a396a64eeac835d25a59323a1b8fd4697424f62374b0de
Participating parties:
                Sui Address                 |                Public Key (Base64)                 | Weight
----------------------------------------------------------------------------------------------------
 0x504f656b7bc467f6eb1d05dc26447477921f05e5ea88c5715682ad28835268ce | ABr818VXt+6PLPRoA7QnsHBfRpKJdWZPjt7ppiTl6Fkq  |   1   
 0x611f6a023c5d1c98b4de96e9da64daffaeb372fed0176536168908e50f6e07c0 | ANRdB4M6Hj73R+gRM4N6zUPNidLuatB9uccOzHBc/0bP  |   1  
 */