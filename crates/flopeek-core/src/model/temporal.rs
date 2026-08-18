//! Temporal observation and exact Context Ref reconciliation contracts.

use super::{ContextRef, GraphBasis};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ObservationContinuityEvent {
    pub event_id: String,
    pub project_id: String,
    pub observation_id: String,
    pub predecessor_event_id: Option<String>,
    pub relation: String,
    pub observed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ObservationContinuity {
    pub schema_version: String,
    pub project_id: String,
    pub current_observation_id: Option<String>,
    pub current_event_id: Option<String>,
    pub current_basis: Option<GraphBasis>,
    pub events: Vec<ObservationContinuityEvent>,
    pub graph_relation: String,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ContextReconciliation {
    pub schema_version: String,
    pub reference: ContextRef,
    pub evaluation_event_id: Option<String>,
    pub status: String,
    pub reason: String,
    pub successor: Option<String>,
    pub candidates: Vec<String>,
    pub truncated: bool,
    pub omissions: Vec<String>,
    pub limitations: Vec<String>,
}
