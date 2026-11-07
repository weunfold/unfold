use super::*;
use serde_json::json;
fn stamp(request: Request) -> Value {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    json!({"v":2,"source":"host","kind":"observe_request","id":format!("rth-front-observe-{}",NEXT.fetch_add(1,Ordering::SeqCst)),"request":request})
}
fn ready(link: &Link) {
    assert!(link.on_frame(&json!({"kind":"observe_ready","schema":SCHEMA,"session":"session"})));
}
fn resume() -> Request {
    Request::Resume {
        session: "session".into(),
    }
}
#[test]
fn negotiation_is_explicit_and_bound_to_hello_session() {
    let link = Link::new("session", stamp);
    assert!(!link.ready());
    assert!(link
        .exchange(resume(), |_| panic!("not negotiated"))
        .is_err());
    assert!(link.on_frame(&json!({"kind":"observe_ready","schema":SCHEMA,"session":"foreign"})));
    ready(&link);
    assert!(
        !link.ready(),
        "invalid negotiation never reopens on same connection"
    );
    let good = Link::new("session", stamp);
    ready(&good);
    assert!(good.ready());
    assert!(!good.on_frame(&json!({"kind":"agent_delta","text":"hello"})));
    assert!(good
        .exchange(
            Request::Resume {
                session: "foreign".into()
            },
            |_| panic!("foreign session")
        )
        .is_err());
}
#[test]
fn reply_correlation_does_not_use_the_next_arbitrary_frame() {
    let link = Link::new("session", stamp);
    ready(&link);
    let reply = link
        .exchange(resume(), |f| {
            assert_eq!(f["kind"], "observe_request");
            assert_eq!(f["request"]["op"], "resume");
            assert!(link.on_frame(
                &json!({"kind":"observe_reply","cause":"other","reply":{"status":"conflict"}})
            ));
            assert!(link.on_frame(
                &json!({"kind":"observe_reply","cause":f["id"],"reply":{"status":"pending"}})
            ));
            Ok(())
        })
        .unwrap();
    assert_eq!(reply, Reply::Pending);
}
#[test]
fn timeout_does_not_leak_a_waiter_or_a_late_refusal_to_the_chat() {
    let link = Link::new("session", stamp);
    ready(&link);
    let mut id = String::new();
    let e = link
        .exchange_with_wait(resume(), Duration::ZERO, |f| {
            id = f["id"].as_str().unwrap().into();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
    assert!(link.on_frame(&json!({"kind":"rejected","cause":id,"reason":"late"})));
    assert!(!link.on_frame(&json!({"kind":"rejected","cause":"unrelated"})));
    assert_eq!(
        link.exchange(resume(), |f| {
            link.on_frame(
                &json!({"kind":"observe_reply","cause":f["id"],"reply":{"status":"pending"}}),
            );
            Ok(())
        })
        .unwrap(),
        Reply::Pending
    );
}
#[test]
fn concurrent_request_is_refused_without_writing_or_replacing_the_waiter() {
    let link = Link::new("session", stamp);
    ready(&link);
    assert_eq!(
        link.exchange(resume(), |f| {
            let e = link
                .exchange(resume(), |_| panic!("second write"))
                .unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::WouldBlock);
            link.on_frame(
                &json!({"kind":"observe_reply","cause":f["id"],"reply":{"status":"pending"}}),
            );
            Ok(())
        })
        .unwrap(),
        Reply::Pending
    );
}
#[test]
fn close_unblocks_waiting_request_and_never_reopens_an_old_attachment() {
    let link = Link::new("session", stamp);
    ready(&link);
    assert!(link
        .exchange(resume(), |_| {
            link.close();
            Ok(())
        })
        .is_err());
    ready(&link);
    assert!(!link.ready());
    assert!(link.exchange(resume(), |_| panic!("closed write")).is_err());
}
#[test]
fn malformed_reply_and_send_failure_do_not_get_accepted() {
    let link = Link::new("session", stamp);
    ready(&link);
    assert!(link.exchange(resume(), |f|{link.on_frame(&json!({"kind":"observe_reply","cause":f["id"],"reply":{"status":"pending","extra":1}}));Ok(())}).is_err());
    assert!(link
        .exchange(resume(), |_| Err(io::Error::other("send failed")))
        .is_err());
    assert_eq!(
        link.exchange(resume(), |f| {
            link.on_frame(
                &json!({"kind":"observe_reply","cause":f["id"],"reply":{"status":"conflict"}}),
            );
            Ok(())
        })
        .unwrap(),
        Reply::Conflict
    );
}

#[test]
fn legacy_acceptance_requires_the_exact_command_and_returns_backend_identity() {
    let link = Link::new("session", stamp);
    ready(&link);
    let result = link.legacy("rth-app:boot:1", "wire-1", || {
        assert!(!link.on_frame(&json!({"kind":"response_started","request_id":"other-command","response_id":"wrong"})));
        assert!(!link.on_frame(&json!({"kind":"command_admitted","command_id":"rth-app:boot:1","duplicate":false,"response_id":"wrong"})));
        assert!(!link.on_frame(&json!({"kind":"response_started","request_id":"rth-app:boot:1","response_id":"backend-response"})));
        Ok(())
    }).unwrap();
    assert_eq!(result, "backend-response");
    assert_ne!(result, "rth-app:boot:1");
}
#[test]
fn legacy_acceptance_supports_exact_duplicate_receipt_but_never_invents_one() {
    let link = Link::new("session", stamp);
    ready(&link);
    let result = link.legacy("command", "wire", || {
        assert!(!link.on_frame(&json!({"kind":"command_admitted","command_id":"foreign","duplicate":true,"response_id":"wrong"})));
        assert!(!link.on_frame(&json!({"kind":"command_admitted","command_id":"command","duplicate":true,"response_id":"backend-duplicate"})));
        Ok(())
    }).unwrap();
    assert_eq!(result, "backend-duplicate");
}
#[test]
fn legacy_timeout_close_refusal_and_concurrency_never_resend() {
    let link = Link::new("session", stamp);
    ready(&link);
    let mut writes = 0;
    let e = link
        .legacy_with_wait("command", "wire", Duration::ZERO, || {
            writes += 1;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::TimedOut);
    assert_eq!(writes, 1);
    assert!(!link
        .on_frame(&json!({"kind":"response_started","request_id":"command","response_id":"late"})));
    let e = link
        .legacy("next", "wire-next", || {
            let busy = link
                .legacy("concurrent", "wire-other", || panic!("second send"))
                .unwrap_err();
            assert_eq!(busy.kind(), io::ErrorKind::WouldBlock);
            link.on_frame(
                &json!({"kind":"rejected","cause":"wire-next","reason":"stale generation"}),
            );
            Ok(())
        })
        .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    let e = link
        .legacy("closed", "wire-closed", || {
            link.close();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::NotConnected);
    assert!(link
        .legacy("after-close", "wire", || panic!("closed send"))
        .is_err());
}
#[test]
fn legacy_receipt_must_have_a_valid_nonempty_backend_response() {
    let link = Link::new("session", stamp);
    ready(&link);
    let e = link
        .legacy_with_wait("command", "wire", Duration::ZERO, || {
            link.on_frame(
                &json!({"kind":"response_started","request_id":"command","response_id":""}),
            );
            Ok(())
        })
        .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    assert_eq!(link.legacy("next", "wire-next", || {
        link.on_frame(&json!({"kind":"response_started","request_id":"next","response_id":"backend-next"})); Ok(())
    }).unwrap(),"backend-next");
}
