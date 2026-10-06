//! One standing interest in published Messages, and who a match goes to.
//!
//! Subscriptions are artifacts: written in TOML, loaded through
//! `xmip-core-configure` and stored through `xmip-core-persist`, like every
//! other artifact.

use serde::{Deserialize, Serialize};

use path::expression::Expression;

/// Who a matched Message goes to.
///
/// Three cases, named rather than encoded in a string. `"SendPort.Billing"`
/// carried the same information and made the reader parse it, which meant a
/// misspelled prefix was a runtime surprise instead of a compile error.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Subscriber {
    WorkProcess(String),
    SendPort(String),
    SendGroup(String),
}

impl Subscriber {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::WorkProcess(name) | Self::SendPort(name) | Self::SendGroup(name) => name,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::WorkProcess(_) => "WorkProcess",
            Self::SendPort(_) => "SendPort",
            Self::SendGroup(_) => "SendGroup",
        }
    }
}

impl std::fmt::Display for Subscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.kind(), self.name())
    }
}

/// One standing interest in published Messages.
///
/// The destination is an artifact — a Send Port, a Send Group or a Work
/// Process. Routing decides *that* a Message goes there, never *how* it gets
/// there.
///
/// `required_contract` and `transformation` name artifacts by id rather than
/// carrying them. A Subscription is configuration, and configuration that
/// embeds the thing it refers to cannot be edited without the thing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub destination: Subscriber,
    /// One line of Xmip's expression language, compiled as it is read
    /// (ADR-0066): a filter that does not compile refuses the document.
    pub filter: Expression,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_contract: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transformation: Option<String>,
}

impl Subscription {
    pub fn new(id: impl Into<String>, destination: Subscriber, filter: Expression) -> Self {
        Self {
            id: id.into(),
            destination,
            filter,
            required_contract: None,
            transformation: None,
        }
    }

    #[must_use]
    pub fn requiring(mut self, contract: impl Into<String>) -> Self {
        self.required_contract = Some(contract.into());
        self
    }

    #[must_use]
    pub fn transforming(mut self, transformation: impl Into<String>) -> Self {
        self.transformation = Some(transformation.into());
        self
    }
}
