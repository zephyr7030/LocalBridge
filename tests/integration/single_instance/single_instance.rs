use super::*;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn names() -> (String, String) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    (
        format!(r"Local\LocalBridge.Test.Single.{nonce}.{}", std::process::id()),
        format!(r"Local\LocalBridge.Test.Wake.{nonce}.{}", std::process::id()),
    )
}

#[test]
fn existing_primary_is_woken_and_releases_single_instance_on_drop() {
    let (mutex, wake) = names();
    let SingleInstanceAcquire::Primary(primary) =
        SingleInstanceGuard::acquire_named(&mutex, &wake).unwrap()
    else {
        panic!("first acquisition must be primary");
    };
    let (tx, rx) = mpsc::channel();
    primary
        .start_wake_listener(move || {
            let _ = tx.send(());
        })
        .unwrap();

    assert!(matches!(
        SingleInstanceGuard::acquire_named(&mutex, &wake).unwrap(),
        SingleInstanceAcquire::Secondary
    ));
    rx.recv_timeout(Duration::from_secs(2))
        .expect("secondary must wake existing primary");
    drop(primary);

    assert!(matches!(
        SingleInstanceGuard::acquire_named(&mutex, &wake).unwrap(),
        SingleInstanceAcquire::Primary(_)
    ));
}
