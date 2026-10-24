use chrono::{Duration, Utc};
use itero::engine::alarm::Alarm;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Instant;

fn alarm() -> (Alarm, Receiver<()>) {
    let (wake, woken) = channel();
    let a = Alarm::start(wake, |e| panic!("the alarm failed: {e}")).unwrap();
    (a, woken)
}

fn ms(n: u64) -> std::time::Duration {
    std::time::Duration::from_millis(n)
}

#[test]
fn it_goes_off_at_the_time_it_is_set_to() {
    let (a, woken) = alarm();
    let started = Instant::now();
    a.set(Some(Utc::now() + Duration::milliseconds(300)))
        .unwrap();
    woken.recv_timeout(ms(3000)).unwrap();
    assert!(started.elapsed() >= ms(250), "{:?}", started.elapsed());
}

#[test]
fn a_time_already_past_goes_off_at_once() {
    let (a, woken) = alarm();
    a.set(Some(Utc::now() - Duration::seconds(5))).unwrap();
    woken.recv_timeout(ms(1000)).unwrap();
}

#[test]
fn set_again_only_the_new_time_goes_off() {
    let (a, woken) = alarm();
    let started = Instant::now();
    a.set(Some(Utc::now() + Duration::milliseconds(200)))
        .unwrap();
    a.set(Some(Utc::now() + Duration::milliseconds(800)))
        .unwrap();
    woken.recv_timeout(ms(3000)).unwrap();
    assert!(started.elapsed() >= ms(700), "{:?}", started.elapsed());
    // and once
    assert_eq!(woken.recv_timeout(ms(500)), Err(RecvTimeoutError::Timeout));
}

#[test]
fn set_to_none_it_does_not_go_off() {
    let (a, woken) = alarm();
    a.set(Some(Utc::now() + Duration::milliseconds(200)))
        .unwrap();
    a.set(None).unwrap();
    assert_eq!(woken.recv_timeout(ms(800)), Err(RecvTimeoutError::Timeout));
}

#[test]
fn it_goes_off_again_once_set_again() {
    let (a, woken) = alarm();
    for _ in 0..3 {
        a.set(Some(Utc::now() + Duration::milliseconds(50)))
            .unwrap();
        woken.recv_timeout(ms(2000)).unwrap();
    }
}

#[test]
fn dropped_its_thread_ends() {
    let (a, woken) = alarm();
    drop(a);
    // the thread held the only sender
    assert_eq!(
        woken.recv_timeout(ms(2000)),
        Err(RecvTimeoutError::Disconnected)
    );
}
