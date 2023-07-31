// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import * as RadixDialog from '@radix-ui/react-dialog';
import { cx } from 'class-variance-authority';
import * as React from 'react';

export const Dialog = RadixDialog.Root;
export const DialogTrigger = RadixDialog.Trigger;

const DialogPortal = ({ className, ...props }: RadixDialog.DialogPortalProps) => (
	<RadixDialog.Portal className={cx(className)} {...props} />
);

const DialogOverlay = React.forwardRef<
	React.ElementRef<typeof RadixDialog.Overlay>,
	React.ComponentPropsWithoutRef<typeof RadixDialog.Overlay>
>(({ className, ...props }, ref) => (
	<RadixDialog.Overlay
		ref={ref}
		className={cx(
			'bg-gray-95/10 backdrop-blur-lg z-[99998] fixed inset-0 bg-background/80 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
			className,
		)}
		{...props}
	/>
));

export const DialogContent = React.forwardRef<
	React.ElementRef<typeof RadixDialog.Content>,
	React.ComponentPropsWithoutRef<typeof RadixDialog.Content>
>(({ className, children, ...props }, ref) => (
	<DialogPortal>
		<DialogOverlay />
		<RadixDialog.Content
			ref={ref}
			className={cx(
				'fixed flex flex-col justify-center z-[99999] left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 shadow-wallet-modal bg-white p-6 rounded-xl w-80 max-w-[85vw] max-h-[60vh] overflow-hidden gap-3 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%]',
				className,
			)}
			{...props}
		>
			{children}
		</RadixDialog.Content>
	</DialogPortal>
));

export const DialogHeader = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
	<div className={cx('flex flex-col gap-1.5 text-center', className)} {...props} />
);

export const DialogFooter = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
	<div className={cx('mt-3', className)} {...props} />
);

export const DialogTitle = React.forwardRef<
	React.ElementRef<typeof RadixDialog.Title>,
	React.ComponentPropsWithoutRef<typeof RadixDialog.Title>
>(({ className, ...props }, ref) => (
	<RadixDialog.Title
		ref={ref}
		className={cx('text-heading6 text-semibold m-0 text-gray-90', className)}
		{...props}
	/>
));

export const DialogDescription = React.forwardRef<
	React.ElementRef<typeof RadixDialog.Description>,
	React.ComponentPropsWithoutRef<typeof RadixDialog.Description>
>(({ className, ...props }, ref) => (
	<RadixDialog.Description
		ref={ref}
		className={cx('text-pBodySmall text-steel', className)}
		{...props}
	/>
));
