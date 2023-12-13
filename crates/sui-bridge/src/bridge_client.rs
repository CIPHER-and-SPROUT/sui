// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `BridgeClient` talks to BridgeNode.

use std::sync::Arc;

use crate::crypto::{verify_signed_bridge_action, BridgeAuthorityPublicKeyBytes};
use crate::error::{BridgeError, BridgeResult};
use crate::server::APPLICATION_JSON;
use crate::types::{BridgeAction, BridgeCommittee, VerifiedSignedBridgeAction};

#[derive(Clone, Debug)]
pub struct BridgeClient {
    inner: reqwest::Client,
    authority: BridgeAuthorityPublicKeyBytes,
    committee: Arc<BridgeCommittee>,
    base_url: String,
}

impl BridgeClient {
    pub fn new<S: Into<String>>(
        base_url: S,
        authority: BridgeAuthorityPublicKeyBytes,
        committee: Arc<BridgeCommittee>,
    ) -> BridgeResult<Self> {
        if !committee.is_active_member(&authority) {
            return Err(BridgeError::InvalidBridgeAuthority(authority));
        }
        Ok(Self {
            inner: reqwest::Client::new(),
            authority,
            base_url: base_url.into(),
            committee,
        })
    }

    #[cfg(test)]
    pub fn update_committee(&mut self, committee: Arc<BridgeCommittee>) {
        self.committee = committee;
    }

    // Important: the paths need to match the ones in server.rs
    fn bridge_action_to_path(event: &BridgeAction) -> String {
        match event {
            BridgeAction::SuiToEthBridgeAction(e) => format!(
                "sign/bridge_tx/sui/eth/{}/{}",
                e.sui_tx_digest, e.sui_tx_event_index
            ),
            // TODO add other events
            _ => unimplemented!(),
        }
    }

    // Returns Ok(true) if the server is up and running
    pub async fn ping(&self) -> BridgeResult<bool> {
        let url = format!("{}/", self.base_url);
        Ok(self
            .inner
            .get(url)
            .header(reqwest::header::ACCEPT, APPLICATION_JSON)
            .send()
            .await?
            .error_for_status()
            .is_ok())
    }

    pub async fn request_sign_bridge_action(
        &self,
        action: BridgeAction,
    ) -> BridgeResult<VerifiedSignedBridgeAction> {
        let url = format!("{}/{}", self.base_url, Self::bridge_action_to_path(&action));

        let resp = self
            .inner
            .get(url)
            .header(reqwest::header::ACCEPT, APPLICATION_JSON)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(BridgeError::RestAPIError(format!(
                "request_sign_bridge_action failed with status: {:?}",
                resp.error_for_status()
            )));
        }
        let signed_bridge_action = resp.json().await?;
        verify_signed_bridge_action(
            &action,
            signed_bridge_action,
            &self.authority,
            &self.committee,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use crate::{
        crypto::BridgeAuthoritySignInfo,
        server::mock_handler::{run_mock_server, BridgeRequestMockHandler},
        test_utils::{get_test_authority_and_key, get_test_sui_to_eth_bridge_action},
        types::SignedBridgeAction,
    };
    use fastcrypto::traits::KeyPair;
    use prometheus::Registry;
    use std::net::IpAddr;
    use std::net::Ipv4Addr;
    use sui_config::local_ip_utils;
    use sui_types::{crypto::get_key_pair, digests::TransactionDigest};

    use super::*;

    #[test]
    fn test_bridge_client() {
        telemetry_subscribers::init_for_testing();

        let (authority, pubkey, _) = get_test_authority_and_key(10000, 12345);
        let pubkey_bytes = BridgeAuthorityPublicKeyBytes::from(&pubkey);
        let committee = Arc::new(BridgeCommittee::new(vec![authority.clone()]).unwrap());

        // Ok
        let _ = BridgeClient::new(
            format!("http://127.0.0.1:{}", 12345),
            pubkey_bytes,
            committee.clone(),
        )
        .unwrap();

        // Err, not in committee
        let (_, kp2): (_, fastcrypto::secp256k1::Secp256k1KeyPair) = get_key_pair();
        let pubkey2_bytes = BridgeAuthorityPublicKeyBytes::from(kp2.public());
        let err = BridgeClient::new(
            format!("http://127.0.0.1:{}", 12345),
            pubkey2_bytes,
            committee.clone(),
        )
        .unwrap_err();
        assert!(matches!(err, BridgeError::InvalidBridgeAuthority(_)));
    }

    #[tokio::test]
    async fn test_bridge_client_request_sign_action() {
        telemetry_subscribers::init_for_testing();
        let registry = Registry::new();
        mysten_metrics::init_metrics(&registry);

        let mock_handler = BridgeRequestMockHandler::new();

        let localhost = local_ip_utils::localhost_for_testing();
        let port = local_ip_utils::get_available_port(&localhost);
        // start server
        let _server_handle = run_mock_server(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port),
            mock_handler.clone(),
        );

        let (authority, _pubkey, secret) = get_test_authority_and_key(5000, port);
        let (authority2, _pubkey2, secret2) = get_test_authority_and_key(5000, port - 1);

        let committee = BridgeCommittee::new(vec![authority.clone(), authority2.clone()]).unwrap();

        let mut client = BridgeClient::new(
            format!("http://127.0.0.1:{}", port),
            authority.pubkey_bytes(),
            Arc::new(committee.clone()),
        )
        .unwrap();

        let tx_digest = TransactionDigest::random();
        let event_idx = 4;

        let action = get_test_sui_to_eth_bridge_action(tx_digest, event_idx, 1, 100);
        let sig = BridgeAuthoritySignInfo::new(&action, &secret);
        let signed_event = SignedBridgeAction::new_from_data_and_sig(action.clone(), sig.clone());
        mock_handler.add_sui_event_response(tx_digest, event_idx, Ok(signed_event.clone()));

        // success
        client
            .request_sign_bridge_action(action.clone())
            .await
            .unwrap();

        // mismatched action would fail, this could happen when the authority fetched the wrong event
        let action2 = get_test_sui_to_eth_bridge_action(tx_digest, event_idx, 2, 200);
        let wrong_sig = BridgeAuthoritySignInfo::new(&action2, &secret);
        let wrong_signed_action =
            SignedBridgeAction::new_from_data_and_sig(action2.clone(), wrong_sig.clone());
        mock_handler.add_sui_event_response(tx_digest, event_idx, Ok(wrong_signed_action));
        let err = client
            .request_sign_bridge_action(action.clone())
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::MismatchedAction));

        // The action matches but the signature is wrong, fail
        let wrong_signed_action =
            SignedBridgeAction::new_from_data_and_sig(action.clone(), wrong_sig);
        mock_handler.add_sui_event_response(tx_digest, event_idx, Ok(wrong_signed_action));
        let err = client
            .request_sign_bridge_action(action.clone())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            BridgeError::InvalidBridgeAuthoritySignature(..)
        ));

        // sig from blocklisted authority would fail
        let mut authority_blocklisted = authority.clone();
        authority_blocklisted.is_blocklisted = true;
        let committee2 = Arc::new(
            BridgeCommittee::new(vec![authority_blocklisted.clone(), authority2.clone()]).unwrap(),
        );
        client.update_committee(committee2);
        mock_handler.add_sui_event_response(tx_digest, event_idx, Ok(signed_event));

        let err = client
            .request_sign_bridge_action(action.clone())
            .await
            .unwrap_err();
        println!("err: {:?}", err);
        assert!(
            matches!(err, BridgeError::InvalidBridgeAuthority(pk) if pk == authority_blocklisted.pubkey_bytes()),
        );

        client.update_committee(committee.into());

        // signed by a different authority in committee would fail
        let sig2 = BridgeAuthoritySignInfo::new(&action, &secret2);
        let signed_event2 = SignedBridgeAction::new_from_data_and_sig(action.clone(), sig2.clone());
        mock_handler.add_sui_event_response(tx_digest, event_idx, Ok(signed_event2));
        let err = client
            .request_sign_bridge_action(action.clone())
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::MismatchedAuthoritySigner));

        // signed by a different key, not in committee, would fail
        let (_, kp3): (_, fastcrypto::secp256k1::Secp256k1KeyPair) = get_key_pair();
        let secret3 = Arc::pin(kp3);
        let sig3 = BridgeAuthoritySignInfo::new(&action, &secret3);
        let signed_event3 = SignedBridgeAction::new_from_data_and_sig(action.clone(), sig3);
        mock_handler.add_sui_event_response(tx_digest, event_idx, Ok(signed_event3));
        let err = client
            .request_sign_bridge_action(action.clone())
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::MismatchedAuthoritySigner));
    }
}
