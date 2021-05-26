use serde::{Deserialize, Serialize};

/// Information passed to DataDog
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DataDogLog {
    /// The message
    pub message: String,
    /// Message tags
    pub ddtags: Option<String>,
    /// Message source
    pub ddsource: String,
    /// Host that sent the message
    pub host: String,
    /// Service that sent the message
    pub service: String,
    /// Datadog understandable string indicating level
    pub level: String,
    #[serde(rename = "dd.trace_id")]
    /// The trace in which this log was generated
    pub trace_id: String,
    #[serde(rename = "dd.span_id")]
    /// The span in which this log was generated
    pub span_id: String,
}
