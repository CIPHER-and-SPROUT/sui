// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::anyhow;
use base64ct::{Base64, Encoding};
use std::env;

use dropshot::{ApiEndpointResponse, HttpError, HttpResponse, CONTENT_TYPE_JSON};
use http::{Response, StatusCode};
use hyper::Body;
use schemars::gen::SchemaGenerator;
use schemars::schema::Schema;
use schemars::{schema_for, JsonSchema};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_with::serde_as;

use sui_types::base_types::{ObjectDigest, ObjectID, ObjectRef, SequenceNumber};
use sui_types::crypto::SignableBytes;
use sui_types::error::SuiError;
use sui_types::messages::TransactionData;
use sui_types::object::{Data, ObjectRead};

#[serde_as]
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ObjectResponse {
    pub objects: Vec<NamedObjectRef>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NamedObjectRef {
    /// Hex code as string representing the object id
    object_id: String,
    /// Object version.
    version: u64,
    /// Base64 string representing the object digest
    digest: String,
}

impl NamedObjectRef {
    pub fn to_object_ref(self) -> Result<ObjectRef, anyhow::Error> {
        Ok((
            ObjectID::try_from(self.object_id)?,
            SequenceNumber::from(self.version),
            ObjectDigest::try_from(&*Base64::decode_vec(&self.digest).map_err(|e| anyhow!(e))?)?,
        ))
    }
}

impl From<ObjectRef> for NamedObjectRef {
    fn from((object_id, version, digest): ObjectRef) -> Self {
        Self {
            object_id: object_id.to_hex(),
            version: version.value(),
            digest: Base64::encode_string(digest.as_ref()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", transparent)]
pub struct JsonResponse<T>(pub T);

impl<T: DeserializeOwned + Serialize> JsonSchema for JsonResponse<T> {
    fn schema_name() -> String {
        serde_name::trace_name::<T>()
            .expect("Self must be a struct or an enum")
            .to_string()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // TODO: Investigate how to extract schema from serde automatically.
        schema_for!(Value).schema.into()
    }
}

pub fn custom_http_error(status_code: http::StatusCode, message: String) -> HttpError {
    HttpError::for_client_error(None, status_code, message)
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TransactionBytes {
    /// Base64 string representation of BCS serialised TransactionData bytes
    tx_bytes: String,
}

impl TransactionBytes {
    pub fn new(data: TransactionData) -> Self {
        Self {
            tx_bytes: Base64::encode_string(&data.to_bytes()),
        }
    }

    pub fn to_data(self) -> Result<TransactionData, anyhow::Error> {
        TransactionData::from_signable_bytes(
            &Base64::decode_vec(&self.tx_bytes).map_err(|e| anyhow!(e))?,
        )
    }
}

/// Response containing the information of an object schema if found, otherwise an error
/// is returned.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectSchemaResponse {
    /// JSON representation of the object schema
    pub schema: serde_json::Value,
}

/// Custom Http Ok response with option to set CORS using env variable
pub struct HttpResponseOk<T: JsonSchema + Serialize + Send + Sync + 'static>(pub T);

impl<T: JsonSchema + Serialize + Send + Sync + 'static> HttpResponse for HttpResponseOk<T> {
    fn to_result(self) -> Result<Response<Body>, HttpError> {
        let body = serde_json::to_string(&self.0)
            .map_err(|err| HttpError::for_internal_error(format!("{err}")))?
            .into();
        let builder = Response::builder()
            .status(StatusCode::OK)
            .header(http::header::CONTENT_TYPE, CONTENT_TYPE_JSON);

        let res = if let Ok(cors) = env::var("ACCESS_CONTROL_ALLOW_ORIGIN") {
            builder.header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, cors)
        } else {
            builder
        }
        .body(body)?;

        Ok(res)
    }

    fn metadata() -> ApiEndpointResponse {
        dropshot::HttpResponseOk::<T>::metadata()
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectExistsResponse {
    object_ref: NamedObjectRef,
    object_type: MoveObjectType,
    object: Value,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectNotExistsResponse {
    object_id: String,
}

#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", content = "details")]
pub enum GetObjectInfoResponse {
    Exists(ObjectExistsResponse),
    NotExists(ObjectNotExistsResponse),
    Deleted(NamedObjectRef),
}

impl TryFrom<ObjectRead> for GetObjectInfoResponse {
    type Error = SuiError;

    fn try_from(obj: ObjectRead) -> Result<Self, Self::Error> {
        match obj {
            ObjectRead::Exists(object_ref, object, layout) => {
                let object_type = MoveObjectType::from_data(&object.data);
                Ok(Self::Exists(ObjectExistsResponse {
                    object_ref: object_ref.into(),
                    object_type,
                    object: object.to_json(&layout)?,
                }))
            }
            ObjectRead::NotExists(object_id) => Ok(Self::NotExists(ObjectNotExistsResponse {
                object_id: object_id.to_hex(),
            })),
            ObjectRead::Deleted(obj_ref) => Ok(Self::Deleted(obj_ref.into())),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MoveObjectType {
    MoveObject,
    MovePackage,
}

impl MoveObjectType {
    fn from_data(data: &Data) -> Self {
        match data {
            Data::Move(_) => MoveObjectType::MoveObject,
            Data::Package(_) => MoveObjectType::MovePackage,
        }
    }
}
