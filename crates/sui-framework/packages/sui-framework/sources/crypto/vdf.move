// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

module sui::vdf {

    /// Hash an arbitrary binary `message` to a class group element to be used as input for `vdf_verify`.
    public fun hash_to_input(message: &vector<u8>): vector<u8> {
        hash_to_input_internal(message)
    }

    /// The internal functions for `hash_to_input`.
    native fun hash_to_input_internal(message: &vector<u8>): vector<u8>;

    /// Verify the output and proof of a VDF with the given number of iterations. The `input`, `output` and `proof`
    /// are all class group elements represented by triples `(a,b,c)` such that `b^2 - 4ac = discriminant`. The are expected
    /// to be encoded as a BCS encoding of a triple of byte arrays, each being the big-endian twos-complement encoding of 
    /// a, b and c in that order.
    /// 
    /// This uses Wesolowski's VDF construction over imaginary class groups as described in Wesolowski (2020), 
    /// 'Efficient Verifiable Delay Functions.', J. Cryptol. 33, and is compatible with the VDF implementation in 
    /// fastcrypto.
    public fun vdf_verify(input: &vector<u8>, output: &vector<u8>, proof: &vector<u8>, iterations: u64): bool {
        vdf_verify_internal(input, output, proof, iterations)
    }

    /// The internal functions for `vdf_verify_internal`. The discriminant must have been verified to be a negative prime which is 1 mod 8.
    native fun vdf_verify_internal(input: &vector<u8>, output: &vector<u8>, proof: &vector<u8>, iterations: u64): bool;
}
