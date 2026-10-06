#![forbid(unsafe_code)]

//! Publication, subscription and dispatch — what BizTalk calls the MessageBox.
//!
//! A Message is published once. Zero or more Subscriptions match it, and each
//! match names a destination. That much Xmip inherits, because it is right.
//!
//! Subscriptions are artifacts. They are written in TOML in an Xmip
//! Application (ADR-0064), loaded through
//! `xmip-core-configure` and stored through `xmip-core-persist`, like every
//! other artifact. Nothing here replaces either.
//!
//! ```toml
//! [[subscriptions]]
//! id = "billing"
//! destination = { send-port = "Billing" }
//! filter = "MessageType = 'Order'"
//!
//! [[subscriptions]]
//! id = "approval"
//! destination = { work-process = "Approval" }
//! filter = "MessageType = 'Order' and Amount > 1000"
//! ```
//!
//! A filter is one line of Xmip's expression language, compiled once when
//! the configuration is read and decided from the compiled tree for every
//! Message (ADR-0066); the language is `xmip-core-path`'s (`path::expression`), and
//! nothing here parses it.
//!
//! What differs from BizTalk is four things, and none of them is the storage.
//!
//! 1. **Matching is a pure function, not a query.** BizTalk evaluates
//!    subscriptions inside SQL Server, so every published Message costs a round
//!    trip to the one MessageBox every node shares. Here the match reads the
//!    promoted set and nothing else, so it runs on the node that holds the
//!    Message, and the same function answers questions at deploy time.
//! 2. **No subscriber is a disposition, not an exception.** BizTalk raises a
//!    routing failure and suspends the Message; you learn about it in
//!    production. Here it is [`Dispatch::Unroutable`], and the Message is kept
//!    for retention.
//! 3. **Every decision explains itself.** [`Routing::declines`] says why each
//!    Subscription passed on the Message. In BizTalk that answer lives in
//!    MessageBox rows you need a separate tool to read.
//! 4. **Promoted values are never guessed.** A promoted value is read as the
//!    kind the filter gives it — the literal it is compared with — and
//!    coercion happens there, in the open.
//!    Inferring that "0012345" is the number 12345 loses a leading zero, and an
//!    order number with it.
//!
//! Merged from the platform repository's `src/route.rs` on 2026-08-26. The
//! behaviour is that file's. `Subscriber` is this module's, and replaces a
//! `destination: String` that encoded the same three cases as text.

mod gathering;
mod never_fires;
mod promoted;
mod routing;
mod source;
mod subscription;

pub use gathering::Gathering;
pub use never_fires::{NeverFires, never_satisfiable};
pub use promoted::{Promoted, routable};
pub use routing::{Dispatch, Evaluation, Routing, publish};
pub use source::{CONTEXT, Reading, Source, SourceError, split};
pub use subscription::{Subscriber, Subscription};

// The tests below stay here rather than moving beside each file. They exercise
// the crate's public surface end to end — publish, filters and the promoted
// set together — over one fixture, which is a different thing from a unit test
// of one file. rust-style.md section 2 is about tests rotting in a distant
// tests/ directory, and these are not distant.

#[cfg(test)]
mod tests {
    use super::*;
    use context::MessageContext;
    use path::expression::{Expression, Truth};
    use xcore::ScalarValue;

    fn orders() -> Promoted {
        Promoted::new()
            .set("MessageType", "Order")
            .set("Amount", "1500")
            .set("Customer", "EU-0042")
            .set("Urgent", "true")
    }

    fn filter(text: &str) -> Expression {
        Expression::parse(text).expect("compiles")
    }

    fn to(id: &str, destination: Subscriber, filter_text: &str) -> Subscription {
        Subscription::new(id, destination, filter(filter_text))
    }

    fn send_port(name: &str) -> Subscriber {
        Subscriber::SendPort(name.to_string())
    }

    fn decided(text: &str, promoted: &Promoted) -> Truth {
        filter(text).evaluate(promoted)
    }

    #[test]
    fn a_matching_subscription_names_its_destination() {
        let subscriptions = vec![to("billing", send_port("Billing"), "MessageType = 'Order'")];
        let routing = publish(&orders(), &subscriptions);

        assert_eq!(routing.destinations(), vec![&send_port("Billing")]);
        assert_eq!(routing.dispatch(), Dispatch::Routed(1));
    }

    #[test]
    fn one_message_can_reach_several_destinations() {
        let subscriptions = vec![
            to("billing", send_port("Billing"), "MessageType = 'Order'"),
            to("archive", send_port("Archive"), "true"),
            to(
                "approval",
                Subscriber::WorkProcess("Approval".to_string()),
                "Amount > 1000",
            ),
        ];
        let routing = publish(&orders(), &subscriptions);

        assert_eq!(routing.destinations().len(), 3);
        assert_eq!(routing.dispatch(), Dispatch::Routed(3));
        assert_eq!(
            routing.destinations()[2].to_string(),
            "WorkProcess.Approval"
        );
    }

    #[test]
    fn nothing_wanting_it_is_a_disposition_and_keeps_the_message() {
        let subscriptions = vec![to(
            "invoices",
            send_port("Invoices"),
            "MessageType = 'Invoice'",
        )];
        let routing = publish(&orders(), &subscriptions);

        assert!(routing.destinations().is_empty());
        assert_eq!(routing.dispatch(), Dispatch::Unroutable);
        assert!(routing.dispatch().retains());
    }

    #[test]
    fn a_decline_says_why() {
        let subscriptions = vec![to(
            "invoices",
            send_port("Invoices"),
            "MessageType = 'Invoice'",
        )];
        let routing = publish(&orders(), &subscriptions);

        assert_eq!(
            routing.declines(),
            vec![("invoices", "MessageType is 'Order', not 'Invoice'")]
        );
    }

    #[test]
    fn a_missing_property_is_unknown_and_named_not_shrugged_at() {
        let subscriptions = vec![to("nordics", send_port("Nordics"), "Region = 'SE'")];
        let routing = publish(&orders(), &subscriptions);

        assert_eq!(
            routing.declines(),
            vec![("nordics", "nothing promoted Region")]
        );
        assert_eq!(
            decided("not Region = 'SE'", &orders()),
            Truth::Unknown("nothing promoted Region".to_string()),
            "not of unknown is unknown, never a silent pass"
        );
    }

    #[test]
    fn the_literal_states_the_kind_and_the_text_is_read_as_it() {
        // Amount was promoted as the text "1500". The literal is an integer,
        // so the comparison is numeric.
        assert!(decided("Amount > 900", &orders()).holds());

        // Lexicographically "1500" is less than "900", which is the wrong
        // answer, and the reason the kind belongs to the filter.
        assert!(!decided("Amount > '900'", &orders()).holds());
    }

    #[test]
    fn a_leading_zero_survives_because_nothing_is_inferred() {
        let promoted = Promoted::new().set("OrderNo", "0012345");

        assert_eq!(promoted.get("OrderNo"), Some("0012345"));
        assert!(decided("OrderNo = '0012345'", &promoted).holds());
    }

    #[test]
    fn text_that_is_not_a_number_is_unknown_with_a_fixable_sentence() {
        let promoted = Promoted::new().set("Amount", "about ten");

        assert_eq!(
            decided("Amount > 5", &promoted),
            Truth::Unknown("Amount is 'about ten', which is not an integer".to_string())
        );
    }

    #[test]
    fn booleans_read_as_written() {
        assert!(decided("Urgent = true", &orders()).holds());
    }

    #[test]
    fn true_takes_everything_and_false_takes_nothing() {
        assert!(decided("true", &orders()).holds());
        assert!(!decided("false", &orders()).holds());
    }

    #[test]
    fn or_reports_every_reason_it_declined() {
        assert_eq!(
            decided("MessageType = 'Invoice' or exists Region", &orders()),
            Truth::False(
                "MessageType is 'Order', not 'Invoice'; and nothing promoted Region".to_string()
            )
        );
    }

    #[test]
    fn not_excludes() {
        assert_eq!(
            decided("exists MessageType and not Customer = 'EU-0042'", &orders()),
            Truth::False("Customer = 'EU-0042' holds".to_string())
        );
    }

    #[test]
    fn like_reads_the_text_as_text() {
        assert!(decided("Customer like 'EU-%'", &orders()).holds());
    }

    #[test]
    fn a_filter_naming_a_property_nothing_promotes_is_found_before_deployment() {
        let promotable = ["MessageType", "Amount", "Customer", "Urgent"];
        let subscriptions = vec![
            to("good", send_port("Billing"), "MessageType = 'Order'"),
            // Regoin, not Region. Neither spelling is promoted.
            to("typo", send_port("Nordics"), "Regoin = 'SE'"),
        ];

        let doomed = never_satisfiable(&promotable, &subscriptions);

        assert_eq!(doomed.len(), 1);
        assert_eq!(doomed[0].subscription_id, "typo");
        assert_eq!(doomed[0].unknown_property, "Regoin");
    }

    #[test]
    fn a_nested_filter_still_gives_up_every_name_it_reads() {
        let nested = filter("(exists A or not B = 1) and C like 'x%'");

        assert_eq!(nested.names(), vec!["A", "B", "C"]);
    }

    #[test]
    fn the_promoted_set_is_ordered_and_the_last_promotion_wins() {
        let promoted = Promoted::new()
            .set("B", "2")
            .set("A", "1")
            .set("A", "1-corrected");

        assert_eq!(promoted.names(), vec!["A", "B"]);
        assert_eq!(promoted.get("A"), Some("1-corrected"));
        assert_eq!(promoted.len(), 2);
    }

    #[test]
    fn promotion_arrives_from_context_as_text() {
        let context = MessageContext::new()
            .with_value("OrderNo", ScalarValue::Text("0012345".into()))
            .with_value("Amount", ScalarValue::Integer(1500))
            .with_value("Urgent", ScalarValue::Bool(true))
            .with_value("Blob", ScalarValue::Binary(vec![0, 1, 2]));
        let message = message::Message::received(
            xcore::MessageId::new(1),
            Vec::new(),
            context,
            message::MessageTreatment::default(),
        );

        let promoted = Gathering::new(&[], &["OrderNo", "Amount", "Urgent"])
            .promote(&message)
            .expect("readable");

        assert_eq!(promoted.get("OrderNo"), Some("0012345"));
        assert_eq!(promoted.get("Amount"), Some("1500"));
        assert_eq!(promoted.get("Urgent"), Some("true"));
        assert_eq!(
            promoted.get("Blob"),
            None,
            "no filter named it, so it is not read"
        );
    }

    #[test]
    fn a_subscription_round_trips_through_toml_as_its_filter_was_written() {
        let subscription = to(
            "approval",
            Subscriber::WorkProcess("Approval".to_string()),
            "Amount>1000",
        )
        .requiring("Order.v2")
        .transforming("OrderToInvoice");

        let text = toml::to_string(&subscription).expect("writing toml");
        assert!(text.contains("filter = \"Amount>1000\""), "{text}");
        let back: Subscription = toml::from_str(&text).expect("reading toml");

        assert_eq!(back, subscription);
        let broken = text.replace("Amount>1000", "Amount > 'x' + 1");
        assert!(
            toml::from_str::<Subscription>(&broken).is_err(),
            "a filter that does not compile is refused as it is read"
        );
    }
}
