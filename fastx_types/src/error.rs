// Copyright (c) Facebook, Inc. and its affiliates.
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

use crate::base_types::*;
use crate::messages::Order;
use move_binary_format::errors::PartialVMError;
use serde::{Deserialize, Serialize};

#[macro_export]
macro_rules! fp_bail {
    ($e:expr) => {
        return Err($e)
    };
}

#[macro_export(local_inner_macros)]
macro_rules! fp_ensure {
    ($cond:expr, $e:expr) => {
        if !($cond) {
            fp_bail!($e);
        }
    };
}
pub(crate) use fp_ensure;

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize, Error, Hash)]
/// Custom error type for FastPay.

#[allow(clippy::large_enum_variant)]
pub enum FastPayError {
    // Signature verification
    #[error("Signature is not valid: {}", error)]
    InvalidSignature { error: String },
    #[error("Value was not signed by the correct sender")]
    IncorrectSigner,
    #[error("Value was not signed by a known authority")]
    UnknownSigner,
    // Certificate verification
    #[error("Signatures in a certificate must form a quorum")]
    CertificateRequiresQuorum,
    // Transfer processing
    #[error("Transfers must have positive amount")]
    IncorrectTransferAmount,
    #[error(
        "The given sequence ({received_sequence:?}) number must match the next expected sequence ({expected_sequence:?}) number of the account"
    )]
    UnexpectedSequenceNumber {
        object_id: ObjectID,
        expected_sequence: SequenceNumber,
        received_sequence: SequenceNumber,
    },
    #[error(
         "The transferred amount must be not exceed the current account balance: {current_balance:?}"
    )]
    InsufficientFunding { current_balance: Balance },
    #[error("Conflicting order already received: {pending_confirmation:?}")]
    ConflictingOrder { pending_confirmation: Order },
    #[error("Transfer order was processed but no signature was produced by authority")]
    ErrorWhileProcessingTransferOrder,
    #[error("An invalid answer was returned by the authority while requesting a certificate")]
    ErrorWhileRequestingCertificate,
    #[error(
         "Cannot confirm a transfer while previous transfer orders are still pending confirmation: {current_sequence_number:?}"
    )]
    MissingEalierConfirmations {
        current_sequence_number: VersionNumber,
    },
    // Synchronization validation
    #[error("Transaction index must increase by one")]
    UnexpectedTransactionIndex,
    // Account access
    #[error("No certificate for this account and sequence number")]
    CertificateNotfound,
    #[error("Unknown sender's account")]
    UnknownSenderAccount,
    #[error("Signatures in a certificate must be from different authorities.")]
    CertificateAuthorityReuse,
    #[error("Sequence numbers above the maximal value are not usable for transfers.")]
    InvalidSequenceNumber,
    #[error("Sequence number overflow.")]
    SequenceOverflow,
    #[error("Sequence number underflow.")]
    SequenceUnderflow,
    #[error("Amount overflow.")]
    AmountOverflow,
    #[error("Amount underflow.")]
    AmountUnderflow,
    #[error("Account balance overflow.")]
    BalanceOverflow,
    #[error("Account balance underflow.")]
    BalanceUnderflow,
    #[error("Wrong shard used.")]
    WrongShard,
    #[error("Invalid cross shard update.")]
    InvalidCrossShardUpdate,
    #[error("Invalid authenticator")]
    InvalidAuthenticator,
    #[error("Cannot deserialize.")]
    InvalidDecoding,
    #[error("Unexpected message.")]
    UnexpectedMessage,
    #[error("The transaction inputs contain duplicates ObjectRef's")]
    DuplicateObjectRefInput,
    #[error("Network error while querying service: {:?}.", error)]
    ClientIoError { error: String },

    // Move module publishing related errors
    #[error("Failed to load the Move module, reason: {error:?}.")]
    ModuleLoadFailure { error: String },
    #[error("Failed to verify the Move module, reason: {error:?}.")]
    ModuleVerificationFailure { error: String },
    #[error("Failed to verify the Move module, reason: {error:?}.")]
    ModuleDeserializationFailure { error: String },
    #[error("Failed to publish the Move module(s), reason: {error:?}.")]
    ModulePublishFailure { error: String },

    // Move call related errors
    #[error("Function resolution failure: {error:?}.")]
    FunctionNotFound { error: String },
    #[error("Function signature is invalid: {error:?}.")]
    InvalidFunctionSignature { error: String },
    #[error("Type error while binding function arguments: {error:?}.")]
    TypeError { error: String },
    #[error("Execution aborted: {error:?}.")]
    AbortedExecution { error: String },

    // Gas related errors
    #[error("Gas budget set higher than max: {error:?}.")]
    GasBudgetTooHigh { error: String },
    #[error("Insufficient gas: {error:?}.")]
    InsufficientGas { error: String },

    // Internal state errors
    #[error("Attempt to re-initialize an order lock.")]
    OrderLockExists,
    #[error("Attempt to set an non-existing order lock.")]
    OrderLockDoesNotExist,
    #[error("Attempt to reset a set order lock to a different value.")]
    OrderLockReset,
    #[error("Could not find the referenced object.")]
    ObjectNotFound,
    #[error("Object ID did not have the expected type")]
    BadObjectType { error: String },
    #[error("Move Execution failed")]
    MoveExecutionFailure,
    #[error("Insufficent input objects")]
    InsufficientObjectNumber,
    #[error("Execution invariant violated")]
    ExecutionInvariantViolation,
    #[error("Storage error")]
    StorageError,
}

pub type FastPayResult<T = ()> = Result<T, FastPayError>;

impl std::convert::From<PartialVMError> for FastPayError {
    fn from(error: PartialVMError) -> Self {
        FastPayError::ModuleVerificationFailure {
            error: error.to_string(),
        }
    }
}
