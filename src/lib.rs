#![forbid(unsafe_code)]

use xmip_contract::ContractDescriptor;
use xmip_message::Message;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Subscriber {
    Process(String),
    SendPort(String),
    SendGroup(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteDecision {
    pub subscriber: Subscriber,
    pub required_contract: Option<ContractDescriptor>,
    pub transformation: Option<String>,
}

pub trait Subscription: Send + Sync {
    fn name(&self) -> &str;
    fn evaluate(&self, message: &Message) -> bool;
    fn decision(&self) -> RouteDecision;
}

pub trait Router: Send + Sync {
    fn route(&self, message: &Message) -> Vec<RouteDecision>;
}

pub struct SubscriptionRouter {
    subscriptions: Vec<Box<dyn Subscription>>,
}

impl SubscriptionRouter {
    pub fn new(subscriptions: Vec<Box<dyn Subscription>>) -> Self {
        Self { subscriptions }
    }
}

impl Router for SubscriptionRouter {
    fn route(&self, message: &Message) -> Vec<RouteDecision> {
        self.subscriptions
            .iter()
            .filter(|subscription| subscription.evaluate(message))
            .map(|subscription| subscription.decision())
            .collect()
    }
}
